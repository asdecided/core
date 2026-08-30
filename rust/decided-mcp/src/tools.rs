//! The six Guide tool bodies over decided-engine's services (ADR-031: the server
//! layer owns no intelligence — it resolves, searches and shapes through the
//! same core functions the CLI uses) with stateless re-read per call
//! (ADR-032: no cache, no session state; identical repository bytes and
//! identical input produce identical output, within the ADR-033 budget).

use crate::graph;
use crate::provenance;
use rac_engine::budget::{
    serialize, HINT_RELATED, MARKER_HINT, MARKER_OMITTED, MARKER_TRUNCATED,
};
use rac_engine::output;
use rac_engine::relationships::corpus_items;
use rac_engine::freshness::TrackerModel;
use rac_engine::resolve::{
    artifact_status, build_index, find_decisions, resolve_in_index, search_index_filtered,
    ResolvedArtifact, SearchResult, OUTCOME_RESOLVED,
};
use serde_json::{json, Map, Value};

fn fixed_origin(
    origin: Option<&rac_engine::corpus::ArtifactOrigin>,
    enabled: bool,
) -> Option<Map<String, Value>> {
    let origin = origin.filter(|_| enabled)?;
    let mut provenance = Map::new();
    provenance.insert("source".to_string(), json!(origin.source));
    provenance.insert("layer".to_string(), json!(origin.layer.as_str()));
    if let Some(pin) = &origin.pin {
        provenance.insert("pin".to_string(), json!(pin));
    }
    Some(provenance)
}

fn attach_origin(
    value: &mut Value,
    origin: Option<&rac_engine::corpus::ArtifactOrigin>,
    enabled: bool,
) {
    let Some(provenance) = fixed_origin(origin, enabled) else {
        return;
    };
    if let Some(record) = value.as_object_mut() {
        record.insert("provenance".to_string(), Value::Object(provenance));
    }
}

fn composed_provenance(
    corpus: &rac_engine::composition::ComposedCorpus,
    key: Option<&rac_engine::corpus::ArtifactKey>,
) -> Option<Map<String, Value>> {
    let provenance = key.and_then(|key| corpus.provenance_for(key))?;
    rac_engine::output::composed_provenance_value(&provenance)
        .as_object()
        .cloned()
}

fn attach_composed_provenance(
    value: &mut Value,
    corpus: &rac_engine::composition::ComposedCorpus,
    key: Option<&rac_engine::corpus::ArtifactKey>,
) {
    let Some(provenance) = composed_provenance(corpus, key) else {
        return;
    };
    if let Some(record) = value.as_object_mut() {
        record.insert("provenance".to_string(), Value::Object(provenance));
    }
}

fn graph_provenance(
    corpus: &rac_engine::graph_federated_corpus::VerifiedGraphCorpus,
    key: Option<&rac_engine::corpus::ArtifactKey>,
) -> Option<Map<String, Value>> {
    let key = key?;
    let item = corpus.composition.item(key)?;
    rac_engine::output::graph_provenance_value(
        &item.origin,
        corpus.composition.provenance_for(key).unwrap_or(&[]),
    )
    .as_object()
    .cloned()
}

fn attach_graph_provenance(
    value: &mut Value,
    corpus: &rac_engine::graph_federated_corpus::VerifiedGraphCorpus,
    key: Option<&rac_engine::corpus::ArtifactKey>,
) {
    let Some(provenance) = graph_provenance(corpus, key) else {
        return;
    };
    if let Some(record) = value.as_object_mut() {
        record.insert("provenance".to_string(), Value::Object(provenance));
    }
}

/// The additive empty-corpus guidance the server layers over the summary.
const EMPTY_GUIDANCE: &str = "This repository has no AsDecided artifacts yet. The user can create the \
first one with `decided quickstart`, or with `decided init` then \
`decided new <type> <path>`. Once artifacts exist, search_artifacts \
and get_artifact will return them.";

fn opt_str(v: &Option<String>) -> Value {
    match v {
        Some(s) => json!(s),
        None => Value::Null,
    }
}

/// `errors.unreadable(id, path)`.
fn unreadable_payload(artifact_id: &str, path: &str) -> Value {
    let mut m = Map::new();
    m.insert("schema_version".to_string(), json!("1"));
    m.insert("error".to_string(), json!("unreadable"));
    m.insert("id".to_string(), json!(artifact_id));
    m.insert("path".to_string(), json!(path));
    Value::Object(m)
}

/// The MCP surfaces always include evidence
/// (`to_dict(include_evidence=True)`); the field map itself is the engine's.
fn artifact_value(m: &ResolvedArtifact) -> Map<String, Value> {
    match output::find_match_value(m, true) {
        Value::Object(obj) => obj,
        _ => unreachable!("find_match_value returns an object"),
    }
}

fn search_result_payload(result: &SearchResult, include_origin: bool) -> Value {
    let mut payload = output::search_result_value(result, true);
    if let Some(matches) = payload
        .as_object_mut()
        .and_then(|object| object.get_mut("matches"))
        .and_then(Value::as_array_mut)
    {
        for (record, artifact) in matches.iter_mut().zip(&result.matches) {
            attach_origin(record, artifact.origin.as_ref(), include_origin);
        }
    }
    payload
}

fn composed_search_result_payload(
    result: &SearchResult,
    corpus: &rac_engine::composition::ComposedCorpus,
) -> Value {
    let mut payload = output::search_result_value(result, true);
    if let Some(matches) = payload
        .as_object_mut()
        .and_then(|object| object.get_mut("matches"))
        .and_then(Value::as_array_mut)
    {
        for (record, artifact) in matches.iter_mut().zip(&result.matches) {
            attach_composed_provenance(record, corpus, artifact.key.as_ref());
        }
    }
    payload
}

fn graph_search_result_payload(
    result: &SearchResult,
    corpus: &rac_engine::graph_federated_corpus::VerifiedGraphCorpus,
) -> Value {
    let mut payload = output::search_result_value(result, true);
    if let Some(matches) = payload
        .as_object_mut()
        .and_then(|object| object.get_mut("matches"))
        .and_then(Value::as_array_mut)
    {
        for (record, artifact) in matches.iter_mut().zip(&result.matches) {
            attach_graph_provenance(record, corpus, artifact.key.as_ref());
        }
    }
    payload
}

/// The per-call budget clamp (ADR-113): a call may only *lower* the server
/// budget; `0` (the default) is the server budget.
pub fn effective_budget(server_budget: i64, call_budget: i64) -> i64 {
    if call_budget <= 0 {
        server_budget
    } else {
        server_budget.min(call_budget)
    }
}

/// Resolve an id through the serving read-model: the mapped base's alias
/// map, the delta snapshot's identity rows, or a fresh walk — every arm
/// byte-identical to `resolve_in_index` (ADR-104).
fn resolve_for(
    root: &str,
    model: Option<&TrackerModel>,
    artifact_id: &str,
) -> rac_engine::resolve::ResolutionResult {
    match model {
        Some(TrackerModel::View(reader)) => {
            rac_engine::read_model::store_resolve(reader, artifact_id)
        }
        Some(TrackerModel::Snapshot(derived)) => {
            let mut result = resolve_in_index(&derived.index_entries, artifact_id);
            if let Some(artifact) = &mut result.artifact {
                // The oracle resolves over the tag-free identity projection.
                artifact.tags = Vec::new();
            }
            result
        }
        Some(TrackerModel::Delta(generation)) => generation.identity.resolve(artifact_id),
        None => resolve_in_index(&build_index(root, true), artifact_id),
    }
}

pub fn get_artifact(
    root: &str,
    model: Option<&TrackerModel>,
    artifact_id: &str,
    budget: i64,
) -> String {
    let result = resolve_for(root, model, artifact_id);
    let Some(artifact) = result
        .artifact
        .as_ref()
        .filter(|_| result.outcome == OUTCOME_RESOLVED)
    else {
        return serialize(&output::resolution_error_value(&result), budget);
    };
    // `None` maps to the `unreadable` structured error (ADR-034).
    let Some(content) = rac_engine::pycompat::read_text_universal(&artifact.path) else {
        return serialize(&unreadable_payload(&artifact.id, &artifact.path), budget);
    };
    let mut payload = Map::new();
    payload.insert("schema_version".to_string(), json!("1"));
    for (k, v) in artifact_value(artifact) {
        payload.insert(k, v);
    }
    let status = artifact_status(&rac_engine::parse::parse_text(&content, &artifact.path));
    let mut prov = fixed_origin(artifact.origin.as_ref(), false).unwrap_or_default();
    prov.insert("status".to_string(), json!(status));
    if artifact
        .origin
        .as_ref()
        .is_none_or(|origin| origin.layer != rac_engine::corpus::Layer::Inherited)
    {
        for (k, v) in provenance::artifact_provenance(root, &artifact.path) {
            prov.insert(k, v);
        }
    }
    // Pinned key order: {schema_version, **artifact, content, provenance}.
    payload.insert("content".to_string(), json!(content));
    payload.insert("provenance".to_string(), Value::Object(prov));
    serialize(&Value::Object(payload), budget)
}

pub fn get_artifact_composed(
    root: &str,
    corpus: &rac_engine::composition::ComposedCorpus,
    artifact_id: &str,
    budget: i64,
) -> String {
    let result = corpus.resolve_identity(artifact_id);
    let Some(artifact) = result
        .artifact
        .as_ref()
        .filter(|_| result.outcome == OUTCOME_RESOLVED)
    else {
        return serialize(&output::resolution_error_value(&result), budget);
    };
    let Some(key) = artifact.key.as_ref() else {
        return serialize(&unreadable_payload(&artifact.id, &artifact.path), budget);
    };
    let Some(content) = corpus
        .content(key)
        .and_then(|bytes| String::from_utf8(bytes.to_vec()).ok())
        .map(|text| text.replace("\r\n", "\n").replace('\r', "\n"))
    else {
        return serialize(&unreadable_payload(&artifact.id, &artifact.path), budget);
    };
    let mut payload = Map::new();
    payload.insert("schema_version".to_string(), json!("1"));
    for (key, value) in artifact_value(artifact) {
        payload.insert(key, value);
    }
    payload.insert("content".to_string(), json!(content));

    let mut provenance = composed_provenance(corpus, Some(key)).unwrap_or_default();
    let status = corpus
        .item(key)
        .map(|item| artifact_status(&item.artifact))
        .unwrap_or_default();
    provenance.insert("status".to_string(), json!(status));
    if let Some(item) = corpus
        .item(key)
        .filter(|item| item.origin.layer == rac_engine::corpus::Layer::Local)
    {
        let physical = item.locator.path.to_string_lossy();
        for (name, value) in provenance::artifact_provenance(root, &physical) {
            provenance.insert(name, value);
        }
    }
    payload.insert("provenance".to_string(), Value::Object(provenance));
    serialize(&Value::Object(payload), budget)
}

pub fn get_artifact_graph(
    root: &str,
    corpus: &rac_engine::graph_federated_corpus::VerifiedGraphCorpus,
    artifact_id: &str,
    budget: i64,
) -> String {
    let result = corpus.composition.resolve_identity(artifact_id);
    let Some(artifact) = result
        .artifact
        .as_ref()
        .filter(|_| result.outcome == OUTCOME_RESOLVED)
    else {
        return serialize(&output::resolution_error_value(&result), budget);
    };
    let Some(key) = artifact.key.as_ref() else {
        return serialize(&unreadable_payload(&artifact.id, &artifact.path), budget);
    };
    let Some(content) = corpus
        .content(key)
        .and_then(|bytes| String::from_utf8(bytes.to_vec()).ok())
        .map(|text| text.replace("\r\n", "\n").replace('\r', "\n"))
    else {
        return serialize(&unreadable_payload(&artifact.id, &artifact.path), budget);
    };
    let mut payload = Map::new();
    payload.insert("schema_version".to_string(), json!("1"));
    for (name, value) in artifact_value(artifact) {
        payload.insert(name, value);
    }
    payload.insert("content".to_string(), json!(content));

    let mut provenance = graph_provenance(corpus, Some(key)).unwrap_or_default();
    let status = corpus
        .composition
        .item(key)
        .map(|item| artifact_status(&item.artifact))
        .unwrap_or_default();
    provenance.insert("status".to_string(), json!(status));
    if let Some(item) = corpus
        .composition
        .item(key)
        .filter(|item| item.origin.layer == rac_engine::corpus::Layer::Local)
    {
        let physical = item.locator.path.to_string_lossy();
        for (name, value) in provenance::artifact_provenance(root, &physical) {
            provenance.insert(name, value);
        }
    }
    payload.insert("provenance".to_string(), Value::Object(provenance));
    serialize(&Value::Object(payload), budget)
}

pub fn search_artifacts(
    root: &str,
    model: Option<&TrackerModel>,
    query: &str,
    artifact_type: Option<&str>,
    tags: &[String],
    live_only: bool,
    budget: i64,
) -> String {
    let mut result = match model {
        Some(TrackerModel::View(reader)) => {
            rac_engine::read_model::store_search(reader, query, artifact_type, tags, live_only)
        }
        Some(TrackerModel::Snapshot(derived)) => {
            search_index_filtered(&derived.index_entries, query, artifact_type, tags, live_only)
        }
        Some(TrackerModel::Delta(generation)) => generation.search.search(
            query,
            artifact_type,
            tags,
            live_only,
            &generation.graph,
        ),
        None => {
            let entries = build_index(root, true);
            search_index_filtered(&entries, query, artifact_type, tags, live_only)
        }
    };
    rac_engine::commands::annotate_search_recency(&mut result.matches, root);
    serialize(
        &search_result_payload(&result, false),
        budget,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn search_artifacts_composed(
    root: &str,
    cached: Option<&rac_engine::derived_cache::ReadModel>,
    corpus: &rac_engine::composition::ComposedCorpus,
    query: &str,
    artifact_type: Option<&str>,
    tags: &[String],
    live_only: bool,
    budget: i64,
) -> String {
    let mut result = match cached {
        Some(rac_engine::derived_cache::ReadModel::View(reader)) => {
            rac_engine::read_model::store_search(
                reader,
                query,
                artifact_type,
                tags,
                live_only,
            )
        }
        Some(rac_engine::derived_cache::ReadModel::Fresh(derived)) => {
            search_index_filtered(
                &derived.index_entries,
                query,
                artifact_type,
                tags,
                live_only,
            )
        }
        None => search_index_filtered(
            &corpus.effective_index(),
            query,
            artifact_type,
            tags,
            live_only,
        ),
    };
    rac_engine::commands::annotate_composed_search_recency(&mut result.matches, root, corpus);
    serialize(&composed_search_result_payload(&result, corpus), budget)
}

#[allow(clippy::too_many_arguments)]
pub fn search_artifacts_graph(
    root: &str,
    model: &rac_engine::derived_cache::ReadModel,
    corpus: &rac_engine::graph_federated_corpus::VerifiedGraphCorpus,
    query: &str,
    artifact_type: Option<&str>,
    tags: &[String],
    live_only: bool,
    budget: i64,
) -> String {
    let mut result = match model {
        rac_engine::derived_cache::ReadModel::View(reader) => rac_engine::read_model::store_search(
            reader,
            query,
            artifact_type,
            tags,
            live_only,
        ),
        rac_engine::derived_cache::ReadModel::Fresh(derived) => search_index_filtered(
            &derived.index_entries,
            query,
            artifact_type,
            tags,
            live_only,
        ),
    };
    rac_engine::commands::annotate_graph_search_recency(&mut result.matches, root, corpus);
    serialize(&graph_search_result_payload(&result, corpus), budget)
}

pub fn find_decisions_tool(
    root: &str,
    model: Option<&TrackerModel>,
    topic: &str,
    path: Option<&str>,
    budget: i64,
) -> String {
    // Python truthiness: a non-empty `path` selects path mode.
    if let Some(p) = path.filter(|p| !p.is_empty()) {
        let include_origin = false;
        // Path mode builds through the same read-model as every other tool
        // (ADR-103), served from precomputed scope rows.
        let payload = match model {
            Some(TrackerModel::View(reader)) => {
                let rows = reader.scope_rows().unwrap_or_default();
                rac_engine::retrieve::scope_lookup_value_with_origin(
                    &rac_engine::retrieve::decisions_for_path_with_rows(&rows, root, p),
                    include_origin,
                )
            }
            Some(TrackerModel::Snapshot(derived)) => {
                rac_engine::retrieve::scope_lookup_value_with_origin(
                    &rac_engine::retrieve::decisions_for_path_with_rows(
                        &derived.scope_rows,
                        root,
                        p,
                    ),
                    include_origin,
                )
            }
            Some(TrackerModel::Delta(generation)) => {
                rac_engine::retrieve::scope_lookup_value_with_origin(
                    &rac_engine::retrieve::decisions_for_path_with_rows(
                        &generation.scope.rows(),
                        root,
                        p,
                    ),
                    include_origin,
                )
            }
            None => rac_engine::retrieve::scope_lookup_value_with_origin(
                &rac_engine::retrieve::decisions_for_path(root, p, true),
                include_origin,
            ),
        };
        return serialize(&payload, budget);
    }
    let result = match model {
        Some(TrackerModel::View(reader)) => {
            rac_engine::read_model::store_find_decisions(reader, topic)
        }
        Some(TrackerModel::Snapshot(derived)) => rac_engine::read_model::find_decisions_in(
            &derived.index_entries,
            &derived.live_decision_paths,
            topic,
        ),
        Some(TrackerModel::Delta(generation)) => rac_engine::read_model::find_decisions_in(
            &generation.search.entries(&generation.graph),
            &generation.scope.live_paths(),
            topic,
        ),
        None => find_decisions(root, topic, true),
    };
    let mut payload = search_result_payload(&result, false);
    payload
        .as_object_mut()
        .expect("object")
        .insert("filter".to_string(), json!("live-decisions"));
    serialize(&payload, budget)
}

pub fn find_decisions_tool_composed(
    root: &str,
    corpus: &rac_engine::composition::ComposedCorpus,
    topic: &str,
    path: Option<&str>,
    budget: i64,
) -> String {
    if let Some(path) = path.filter(|path| !path.is_empty()) {
        let items: Vec<_> = corpus.effective().cloned().collect();
        let rows = rac_engine::retrieve::scope_rows_from_items(&items);
        let result = rac_engine::retrieve::decisions_for_path_with_rows(&rows, root, path);
        return serialize(
            &rac_engine::retrieve::scope_lookup_value_with_composed(&result, corpus),
            budget,
        );
    }

    let entries = corpus.effective_index();
    let live: std::collections::HashSet<_> = corpus
        .effective()
        .filter(|item| {
            item.spec.map(|spec| spec.name.as_str()) == Some("decision")
                && rac_engine::resolve::is_live_decision(&item.artifact)
        })
        .map(|item| item.key.clone())
        .collect();
    let mut result = search_index_filtered(&entries, topic, Some("decision"), &[], false);
    result
        .matches
        .retain(|artifact| artifact.key.as_ref().is_some_and(|key| live.contains(key)));
    let mut payload = composed_search_result_payload(&result, corpus);
    payload
        .as_object_mut()
        .expect("object")
        .insert("filter".to_string(), json!("live-decisions"));
    serialize(&payload, budget)
}

pub fn find_decisions_tool_graph(
    root: &str,
    model: &rac_engine::derived_cache::ReadModel,
    corpus: &rac_engine::graph_federated_corpus::VerifiedGraphCorpus,
    topic: &str,
    path: Option<&str>,
    budget: i64,
) -> String {
    if let Some(path) = path.filter(|path| !path.is_empty()) {
        let rows = match model {
            rac_engine::derived_cache::ReadModel::View(reader) => {
                reader.scope_rows().unwrap_or_default()
            }
            rac_engine::derived_cache::ReadModel::Fresh(derived) => derived.scope_rows.clone(),
        };
        let result = rac_engine::retrieve::decisions_for_path_with_rows(&rows, root, path);
        let mut payload = rac_engine::retrieve::scope_lookup_value_with_origin(&result, true);
        if let Some(decisions) = payload
            .get_mut("decisions")
            .and_then(Value::as_array_mut)
        {
            for (value, decision) in decisions.iter_mut().zip(&result.decisions) {
                attach_graph_provenance(value, corpus, decision.key.as_ref());
            }
        }
        return serialize(&payload, budget);
    }

    let result = match model {
        rac_engine::derived_cache::ReadModel::View(reader) => {
            rac_engine::read_model::store_find_decisions(reader, topic)
        }
        rac_engine::derived_cache::ReadModel::Fresh(derived) => {
            rac_engine::read_model::find_decisions_in_source_aware(
                &derived.index_entries,
                &derived.live_decision_keys,
                &derived.live_decision_paths,
                topic,
            )
        }
    };
    let mut payload = graph_search_result_payload(&result, corpus);
    payload
        .as_object_mut()
        .expect("object")
        .insert("filter".to_string(), json!("live-decisions"));
    serialize(&payload, budget)
}

pub fn get_related(
    graph_view: &graph::GraphView,
    artifact_id: &str,
    depth: i64,
    budget: i64,
) -> String {
    get_related_inner(graph_view, None, None, artifact_id, depth, budget)
}

pub fn get_related_composed(
    graph_view: &graph::GraphView,
    corpus: &rac_engine::composition::ComposedCorpus,
    artifact_id: &str,
    depth: i64,
    budget: i64,
) -> String {
    get_related_inner(graph_view, Some(corpus), None, artifact_id, depth, budget)
}

pub fn get_related_graph(
    graph_view: &graph::GraphView,
    corpus: &rac_engine::graph_federated_corpus::VerifiedGraphCorpus,
    artifact_id: &str,
    depth: i64,
    budget: i64,
) -> String {
    get_related_inner(graph_view, None, Some(corpus), artifact_id, depth, budget)
}

fn get_related_inner(
    graph_view: &graph::GraphView,
    corpus: Option<&rac_engine::composition::ComposedCorpus>,
    graph_corpus: Option<&rac_engine::graph_federated_corpus::VerifiedGraphCorpus>,
    artifact_id: &str,
    depth: i64,
    budget: i64,
) -> String {
    let graph_started = rac_engine::timing::start();
    let result = if let Some(corpus) = graph_corpus {
        corpus.composition.resolve_identity(artifact_id)
    } else if let Some(corpus) = corpus {
        corpus.resolve_identity(artifact_id)
    } else {
        graph_view.resolve(artifact_id)
    };
    let Some(artifact) = result
        .artifact
        .as_ref()
        .filter(|_| result.outcome == OUTCOME_RESOLVED)
    else {
        return serialize(&output::resolution_error_value(&result), budget);
    };
    let include_origin = graph_view.is_federated();
    let historical = if let Some(corpus) = graph_corpus {
        artifact.key.as_ref().is_some_and(|key| {
            artifact_id
                .split_once("::")
                .is_some_and(|(qualifier, _)| qualifier == key.source)
                && corpus.composition.terminal_redirects().contains_key(key)
        })
    } else {
        corpus.is_some_and(|corpus| {
            artifact
                .key
                .as_ref()
                .is_some_and(|key| corpus.is_overridden(key))
        })
    };
    let outgoing = graph_view.outgoing(artifact, historical);
    let incoming_result = graph_view.incoming(artifact, historical);
    let incoming: Vec<Value> = incoming_result
        .items
        .iter()
        .map(|r| {
            let mut m = Map::new();
            m.insert("id".to_string(), json!(r.id));
            m.insert("type".to_string(), json!(r.artifact_type));
            m.insert("title".to_string(), opt_str(&r.title));
            m.insert("path".to_string(), json!(r.path));
            m.insert("section".to_string(), json!(r.section));
            let mut ev = Map::new();
            ev.insert("direction".to_string(), json!("incoming"));
            ev.insert("relationship".to_string(), json!(r.section));
            ev.insert("target".to_string(), json!(r.target));
            m.insert("evidence".to_string(), Value::Object(ev));
            let mut value = Value::Object(m);
            if let Some(corpus) = corpus {
                attach_composed_provenance(&mut value, corpus, r.key.as_ref());
            } else if let Some(corpus) = graph_corpus {
                attach_graph_provenance(&mut value, corpus, r.key.as_ref());
            } else {
                attach_origin(&mut value, r.origin.as_ref(), include_origin);
            }
            value
        })
        .collect();
    let mut payload = Map::new();
    payload.insert("schema_version".to_string(), json!("1"));
    for (k, v) in artifact_value(artifact) {
        payload.insert(k, v);
    }
    if let Some(provenance) = corpus
        .and_then(|corpus| composed_provenance(corpus, artifact.key.as_ref()))
        .or_else(|| graph_corpus.and_then(|corpus| graph_provenance(corpus, artifact.key.as_ref())))
        .or_else(|| fixed_origin(artifact.origin.as_ref(), include_origin))
    {
        payload.insert("provenance".to_string(), Value::Object(provenance));
    }
    payload.insert("outgoing".to_string(), outgoing.to_value());
    payload.insert("incoming".to_string(), Value::Array(incoming));
    let mut neighborhood_truncated = false;
    if depth > 1 {
        let hood = graph_view.neighborhood(artifact, depth, historical);
        let nodes: Vec<Value> = hood
            .nodes
            .iter()
            .filter(|n| n.hops > 1)
            .map(|n| {
                let mut m = Map::new();
                m.insert("id".to_string(), json!(n.id));
                m.insert("type".to_string(), json!(n.artifact_type));
                m.insert("title".to_string(), opt_str(&n.title));
                m.insert("path".to_string(), json!(n.path));
                m.insert("hops".to_string(), json!(n.hops));
                let mut value = Value::Object(m);
                if let Some(corpus) = corpus {
                    attach_composed_provenance(&mut value, corpus, n.key.as_ref());
                } else if let Some(corpus) = graph_corpus {
                    attach_graph_provenance(&mut value, corpus, n.key.as_ref());
                } else {
                    attach_origin(&mut value, n.origin.as_ref(), include_origin);
                }
                value
            })
            .collect();
        payload.insert("neighborhood".to_string(), Value::Array(nodes));
        payload.insert(
            "depth".to_string(),
            json!(depth.min(graph::MAX_TRAVERSAL_DEPTH)),
        );
        neighborhood_truncated = hood.truncated;
    }
    let edge_overflow = (incoming_result.total - incoming_result.items.len())
        + (outgoing.total - outgoing.kept());
    if edge_overflow > 0 || neighborhood_truncated {
        payload.insert(MARKER_TRUNCATED.to_string(), json!(true));
        payload.insert(MARKER_OMITTED.to_string(), json!(edge_overflow as i64));
        payload.insert(MARKER_HINT.to_string(), json!(HINT_RELATED));
    }
    rac_engine::timing::emit_since(
        "graph.lookup",
        graph_started,
        &[
            ("incoming", incoming_result.total as u64),
            ("outgoing", outgoing.total as u64),
            ("depth", depth.max(0) as u64),
        ],
    );
    serialize(&Value::Object(payload), budget)
}

pub fn get_summary(root: &str, model: Option<&TrackerModel>, budget: i64) -> String {
    // Served from the read-model's precomputed portfolio summary when the
    // tracker is on (ADR-103); the guidance key stays additive (a shallow
    // copy — the cached bundle is never mutated).
    if let Some(model) = model {
        let summary = match model {
            TrackerModel::View(reader) => reader.portfolio_summary().unwrap_or(Value::Null),
            TrackerModel::Snapshot(derived) => derived.portfolio_summary.clone(),
            TrackerModel::Delta(generation) => generation.summary.value(root, true),
        };
        if let Value::Object(mut payload) = summary {
            let empty = payload
                .get("empty")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            if empty {
                payload.insert("guidance".to_string(), json!(EMPTY_GUIDANCE));
            }
            return serialize(&Value::Object(payload), budget);
        }
        // A malformed cached summary degrades to the fresh walk below.
    }
    let corpus = corpus_items(root, true);
    let p = rac_engine::portfolio::portfolio_from_corpus(root, &corpus, true);
    let mut payload = Map::new();
    payload.insert("schema_version".to_string(), json!("1"));
    payload.insert("directory".to_string(), json!(p.directory));
    payload.insert("recursive".to_string(), json!(p.recursive));
    let empty = p.total_artifacts() == 0;
    payload.insert("empty".to_string(), json!(empty));
    let mut by_type = Map::new();
    for (t, c) in &p.by_type {
        by_type.insert(t.clone(), json!(c));
    }
    let mut artifacts = Map::new();
    artifacts.insert("total".to_string(), json!(p.total_artifacts()));
    artifacts.insert("by_type".to_string(), Value::Object(by_type));
    artifacts.insert("unknown_paths".to_string(), json!(p.unknown_paths));
    payload.insert("artifacts".to_string(), Value::Object(artifacts));
    let mut validation = Map::new();
    validation.insert("valid".to_string(), json!(p.valid_artifacts));
    validation.insert("invalid".to_string(), json!(p.invalid_artifacts));
    payload.insert("validation".to_string(), Value::Object(validation));
    let mut completeness = Map::new();
    completeness.insert("recommended_slots".to_string(), json!(p.recommended_slots));
    completeness.insert("filled".to_string(), json!(p.filled_slots));
    completeness.insert(
        "ratio".to_string(),
        rac_engine::pyjson::py_float(p.completeness()),
    );
    payload.insert("completeness".to_string(), Value::Object(completeness));
    let mut relationships = Map::new();
    relationships.insert("total".to_string(), json!(p.relationships.total));
    relationships.insert("valid".to_string(), json!(p.relationships.valid));
    relationships.insert("broken".to_string(), json!(p.relationships.broken));
    relationships.insert("orphaned".to_string(), json!(p.relationships.orphaned));
    relationships.insert(
        "coverage".to_string(),
        rac_engine::pyjson::py_float(p.relationships.coverage),
    );
    payload.insert("relationships".to_string(), Value::Object(relationships));
    let attention: Vec<Value> = p
        .attention
        .iter()
        .map(|item| {
            let mut m = Map::new();
            m.insert("path".to_string(), json!(item.path));
            m.insert("identifier".to_string(), json!(item.identifier));
            m.insert("severity".to_string(), json!(item.severity));
            m.insert("code".to_string(), json!(item.code));
            m.insert("message".to_string(), json!(item.message));
            Value::Object(m)
        })
        .collect();
    payload.insert("attention".to_string(), Value::Array(attention));
    let mut health = Map::new();
    health.insert("score".to_string(), json!(p.health_score()));
    payload.insert("health".to_string(), Value::Object(health));
    let mut status = Map::new();
    status.insert("artifacts_ok".to_string(), json!(p.invalid_artifacts == 0));
    status.insert("relationships_ok".to_string(), json!(p.relationships_ok));
    status.insert(
        "ok".to_string(),
        json!(p.invalid_artifacts == 0 && p.relationships_ok),
    );
    payload.insert("validation_status".to_string(), Value::Object(status));
    if empty {
        payload.insert("guidance".to_string(), json!(EMPTY_GUIDANCE));
    }
    serialize(&Value::Object(payload), budget)
}

pub fn get_summary_composed(
    root: &str,
    generation: &rac_engine::derived_cache::LogicalGeneration,
    corpus: &rac_engine::composition::ComposedCorpus,
    budget: i64,
) -> String {
    let identity = generation
        .identity()
        .expect("federated request has generation identity");
    let parent = generation
        .verified_parent()
        .expect("federated request has verified parent");
    let items: Vec<_> = corpus.effective().cloned().collect();
    let overrides = rac_engine::validate::overrides_from_config_bytes(&parent.child_config_bytes);
    let summary = rac_engine::portfolio::portfolio_from_corpus_with_analysis(
        &identity.child_corpus_path,
        &items,
        identity.recursive,
        &overrides,
        corpus.relationship_summary(),
        corpus.validate_relationships(root, identity.recursive).ok(),
    );
    let mut payload = rac_engine::output::portfolio_summary_value(&summary);
    if payload
        .get("empty")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        payload
            .as_object_mut()
            .expect("portfolio payload is an object")
            .insert("guidance".to_string(), json!(EMPTY_GUIDANCE));
    }
    serialize(&payload, budget)
}

pub fn get_summary_graph(
    model: &rac_engine::derived_cache::ReadModel,
    budget: i64,
) -> String {
    let summary = match model {
        rac_engine::derived_cache::ReadModel::View(reader) => {
            reader.portfolio_summary().unwrap_or(Value::Null)
        }
        rac_engine::derived_cache::ReadModel::Fresh(derived) => {
            derived.portfolio_summary.clone()
        }
    };
    let mut payload = match summary {
        Value::Object(payload) => payload,
        _ => Map::new(),
    };
    if payload
        .get("empty")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        payload.insert("guidance".to_string(), json!(EMPTY_GUIDANCE));
    }
    serialize(&Value::Object(payload), budget)
}

pub fn retrieve_grounding(
    root: &str,
    model: Option<&TrackerModel>,
    task: &str,
    scope: &str,
    top_k: i64,
    effective: i64,
    live_only: bool,
) -> String {
    // Python passes `scope or None`; the engine's own empty filter matches.
    let scope_opt = if scope.is_empty() { None } else { Some(scope) };
    let payload = match model {
        Some(TrackerModel::View(reader)) => rac_engine::retrieve::retrieve_grounding_from_store(
            root, task, scope_opt, top_k, effective, live_only, reader,
        ),
        Some(TrackerModel::Snapshot(derived)) => {
            rac_engine::retrieve::retrieve_grounding_from_derived(
                root, task, scope_opt, top_k, effective, live_only, derived,
            )
        }
        Some(TrackerModel::Delta(generation)) => {
            let derived = generation.materialize_derived(root, true);
            rac_engine::retrieve::retrieve_grounding_from_derived(
                root,
                task,
                scope_opt,
                top_k,
                effective,
                live_only,
                &derived,
            )
        }
        None => rac_engine::retrieve::retrieve_grounding(
            root, task, scope_opt, top_k, effective, live_only,
        ),
    };
    serialize(&payload, effective)
}

pub fn retrieve_grounding_composed(
    root: &str,
    corpus: &rac_engine::composition::ComposedCorpus,
    task: &str,
    scope: &str,
    top_k: i64,
    effective: i64,
    live_only: bool,
) -> String {
    let scope = if scope.is_empty() { None } else { Some(scope) };
    let payload = rac_engine::retrieve::retrieve_grounding_from_composed(
        root,
        task,
        scope,
        top_k,
        effective,
        live_only,
        corpus,
    );
    serialize(&payload, effective)
}

pub fn retrieve_grounding_graph(
    root: &str,
    corpus: &rac_engine::graph_federated_corpus::VerifiedGraphCorpus,
    task: &str,
    scope: &str,
    top_k: i64,
    effective: i64,
    live_only: bool,
) -> String {
    let scope = if scope.is_empty() { None } else { Some(scope) };
    let payload = rac_engine::retrieve::retrieve_grounding_from_graph(
        root,
        task,
        scope,
        top_k,
        effective,
        live_only,
        corpus,
    );
    serialize(&payload, effective)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rac_engine::delta_generation::{DeltaGeneration, IdentityGeneration};
    use rac_engine::freshness::TrackerModel;

    fn decision(id: &str) -> String {
        format!(
            "---\nschema_version: 1\nid: {id}\ntype: decision\n---\n# {id}: Identity\n\n## Context\n\nTest.\n\n## Decision\n\nKeep.\n\n## Consequences\n\nNone.\n\n## Status\n\nAccepted\n"
        )
    }

    #[test]
    fn fixed_origin_is_additive_only_in_a_federated_context() {
        let local = rac_engine::corpus::CorpusLayer::local("acme/app").origin();
        assert!(fixed_origin(Some(&local), false).is_none());
        assert_eq!(
            Value::Object(fixed_origin(Some(&local), true).unwrap()),
            json!({"source": "acme/app", "layer": "local"})
        );

        let inherited = rac_engine::corpus::CorpusLayer::inherited(
            "acme/standards",
            "standards",
            "sha256:0123",
        )
        .origin();
        assert_eq!(
            Value::Object(fixed_origin(Some(&inherited), true).unwrap()),
            json!({
                "source": "acme/standards",
                "layer": "inherited",
                "pin": "sha256:0123"
            })
        );
    }

    #[test]
    fn delta_point_and_search_routes_use_incremental_generations() {
        let root =
            std::env::temp_dir().join(format!("decided-p6-identity-route-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("decision.md");
        std::fs::write(&path, decision("RAC-111111111111")).unwrap();
        let root_str = root.to_string_lossy().into_owned();
        std::fs::write(&path, decision("RAC-222222222222")).unwrap();
        let items = rac_engine::relationships::corpus_items(&root_str, true);
        let identity =
            IdentityGeneration::from_items(items.iter().map(|item| ("decision.md", item)));
        let model = TrackerModel::Delta(Box::new(DeltaGeneration {
            base_generation: 1,
            serving_generation: 2,
            changed_paths: vec!["decision.md".to_string()],
            identity,
            search: rac_engine::delta_generation::SearchGeneration::from_items(
                items.iter().map(|item| ("decision.md", item)),
            ),
            graph: rac_engine::delta_generation::GraphGeneration::from_items(
                items.iter().map(|item| ("decision.md", item)),
                &IdentityGeneration::from_items(items.iter().map(|item| ("decision.md", item))),
            ),
            scope: rac_engine::delta_generation::ScopeGeneration::from_items(
                items.iter().map(|item| ("decision.md", item)),
            ),
            summary: rac_engine::delta_generation::SummaryGeneration::from_items(
                items.iter().map(|item| ("decision.md", item)),
            ),
        }));

        assert_eq!(
            resolve_for(&root_str, Some(&model), "RAC-222222222222").outcome,
            rac_engine::resolve::OUTCOME_RESOLVED
        );
        assert_eq!(
            resolve_for(&root_str, Some(&model), "RAC-111111111111").outcome,
            rac_engine::resolve::OUTCOME_NOT_FOUND
        );
        let search = search_artifacts(
            &root_str,
            Some(&model),
            "222222222222",
            None,
            &[],
            false,
            16_384,
        );
        assert!(search.contains("RAC-222222222222"));
        assert!(!search.contains("RAC-111111111111"));
        let _ = std::fs::remove_dir_all(root);
    }
}
