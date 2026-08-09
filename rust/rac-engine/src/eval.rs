//! Grounding retrieval benchmark — `decided eval` (PORT-CONTRACT.d/15).
//!
//! Port of `src/asdecided/services/eval.py`. Deterministic by ADR-066: the scored
//! path is a pure function of (corpus bytes, query set, retrieval code) — no
//! network, no randomness, no clock. The only wall-clock/build values are
//! `metadata.generated_at` and `metadata.lore_version`, both diagnostic and
//! excluded from the gate (the parity harness masks them in `--json`).
//!
//! The benchmark guards the REAL retrieval surface: a `search_artifacts`
//! case consumes `resolve::search_index` order verbatim, and a `get_related`
//! case consumes the `incoming` neighborhood ordering that the MCP
//! `get_related` tool serializes (mirrored here from `decided-mcp::graph::
//! incoming_references` — decided-engine cannot depend on decided-mcp, and eval only
//! needs the ordered id list).

use std::collections::HashMap;
use std::path::Path;

use serde_json::{Map, Value};

use crate::composition::ComposedCorpus;
use crate::corpus::ArtifactPath;
use crate::pycompat::{py_repr_str, py_round};
use crate::pyjson::py_float;
use crate::relationships::{corpus_items, relationships_from_corpus, Relationship};
use crate::resolve::{
    build_index, index_from_items, resolve_in_index, search_index, IndexEntry, OUTCOME_RESOLVED,
};
use crate::sha256::Sha256;
use crate::spec::{snake, RELATIONSHIP_SECTIONS};
use crate::walk::find_markdown_files;

/// The ranks the benchmark reports Precision@k / Recall@k at (REQ-003).
pub const K_VALUES: [usize; 3] = [1, 3, 5];
/// The hard-negative window: the widest k (REQ-003).
pub const NEGATIVE_K: usize = 5;
/// Metric rounding precision (`_PRECISION`).
const PRECISION: i32 = 6;

pub const DEFAULT_CORPUS: &str = "rust/fixtures/eval/corpus";
pub const DEFAULT_QUERIES: &str = "rust/fixtures/eval/queries.json";
pub const DEFAULT_BASELINE: &str = "rust/fixtures/eval/baseline.json";
pub const DEFAULT_CONFIG: &str = "rust/fixtures/eval/eval-config.json";

const TOOL_SEARCH: &str = "search_artifacts";
const TOOL_GET_RELATED: &str = "get_related";

/// `EvalUsageError` — the CLI maps this to exit 2 (`decided eval: <msg>`).
pub struct EvalUsageError(pub String);

type EvalResult<T> = Result<T, EvalUsageError>;

fn usage<T>(message: String) -> EvalResult<T> {
    Err(EvalUsageError(message))
}

/// One scored retrieval case (REQ-008).
pub struct QueryCase {
    pub id: String,
    pub tool: String,
    pub query: String,
    pub category: String,
    pub relevant: Vec<String>,
    pub must_not_return: Vec<String>,
    /// Optional artifact-type filter, search cases only.
    pub artifact_type: Option<String>,
}

/// The scored outcome of one case — a `per_query` row.
struct CaseResult {
    case: QueryCase,
    returned: Vec<String>,
    /// Indexed like K_VALUES.
    precision: [f64; 3],
    recall: [f64; 3],
    violations: Vec<String>,
}

impl CaseResult {
    fn to_value(&self) -> Value {
        let mut m = Map::new();
        m.insert("id".into(), Value::String(self.case.id.clone()));
        m.insert("tool".into(), Value::String(self.case.tool.clone()));
        m.insert("category".into(), Value::String(self.case.category.clone()));
        m.insert("returned".into(), str_list(&self.returned));
        m.insert("relevant".into(), str_list(&self.case.relevant));
        if !self.case.must_not_return.is_empty() {
            m.insert("must_not_return".into(), str_list(&self.case.must_not_return));
        }
        for (i, k) in K_VALUES.iter().enumerate() {
            m.insert(format!("p_at_{k}"), py_float(round6(self.precision[i])));
        }
        for (i, k) in K_VALUES.iter().enumerate() {
            m.insert(format!("r_at_{k}"), py_float(round6(self.recall[i])));
        }
        m.insert("violations".into(), str_list(&self.violations));
        Value::Object(m)
    }
}

fn str_list(items: &[String]) -> Value {
    Value::Array(items.iter().map(|s| Value::String(s.clone())).collect())
}

/// A full benchmark run: gated `metrics` plus diagnostic context.
pub struct Scorecard {
    pub metrics: Value,
    pub metadata: Value,
    pub per_query: Vec<Value>,
}

impl Scorecard {
    fn to_value(&self) -> Value {
        let mut m = Map::new();
        m.insert("metrics".into(), self.metrics.clone());
        m.insert("metadata".into(), self.metadata.clone());
        m.insert("per_query".into(), Value::Array(self.per_query.clone()));
        Value::Object(m)
    }
}

fn round6(value: f64) -> f64 {
    py_round(value, PRECISION)
}

// --- Loading committed inputs (usage errors → EvalUsageError) ----------------

fn load_json(path: &str, what: &str) -> EvalResult<Value> {
    if !Path::new(path).is_file() {
        return usage(format!("{what} not found: {path}"));
    }
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) => return usage(format!("cannot read {what}: {path}: {e}")),
    };
    let text = match String::from_utf8(bytes) {
        Ok(t) => t,
        Err(e) => return usage(format!("cannot read {what}: {path}: {e}")),
    };
    match serde_json::from_str::<Value>(&text) {
        Ok(v) => Ok(v),
        // The oracle embeds CPython's JSONDecodeError text here; serde's
        // message differs (stderr-only surface, never byte-refereed).
        Err(e) => usage(format!("malformed {what}: {path}: {e}")),
    }
}

/// `str(x)` over the JSON scalars a query set can carry.
fn py_str(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Bool(true) => "True".to_string(),
        Value::Bool(false) => "False".to_string(),
        Value::Null => "None".to_string(),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                i.to_string()
            } else {
                crate::pycompat::py_float_repr(n.as_f64().unwrap_or(0.0))
            }
        }
        other => other.to_string(),
    }
}

/// `load_query_set(path)` — parse and shape-check the committed query set.
pub fn load_query_set(path: &str) -> EvalResult<Vec<QueryCase>> {
    let data = load_json(path, "query set")?;
    // `data.get("cases") if isinstance(data, dict) else data`
    let cases_raw = match &data {
        Value::Object(map) => map.get("cases").cloned().unwrap_or(Value::Null),
        other => other.clone(),
    };
    let Value::Array(cases_raw) = cases_raw else {
        return usage(format!(
            "malformed query set: {path}: expected a non-empty 'cases' list"
        ));
    };
    if cases_raw.is_empty() {
        return usage(format!(
            "malformed query set: {path}: expected a non-empty 'cases' list"
        ));
    }
    let mut cases: Vec<QueryCase> = Vec::with_capacity(cases_raw.len());
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    for (i, raw) in cases_raw.iter().enumerate() {
        let case = parse_case(raw, path, i)?;
        if !seen.insert(case.id.clone()) {
            return usage(format!(
                "malformed query set: {path}: duplicate case id {}",
                py_repr_str(&case.id)
            ));
        }
        cases.push(case);
    }
    Ok(cases)
}

fn parse_case(raw: &Value, path: &str, index: usize) -> EvalResult<QueryCase> {
    let Value::Object(map) = raw else {
        return usage(format!("malformed query set: {path}: case {index} is not an object"));
    };
    let require = |field: &str| -> EvalResult<&Value> {
        map.get(field).ok_or_else(|| {
            EvalUsageError(format!(
                "malformed query set: {path}: case {index} missing {}",
                py_repr_str(field)
            ))
        })
    };
    let case_id = require("id")?.clone();
    let tool = require("tool")?.clone();
    let query = require("query")?.clone();
    let category = require("category")?.clone();
    let relevant = require("relevant")?.clone();
    let id_repr = py_repr_str(&py_str(&case_id));
    if py_str(&tool) != TOOL_SEARCH && py_str(&tool) != TOOL_GET_RELATED {
        return usage(format!(
            "malformed query set: {path}: case {id_repr} tool must be one of ('{TOOL_SEARCH}', '{TOOL_GET_RELATED}')"
        ));
    }
    let Value::Array(relevant) = relevant else {
        return usage(format!(
            "malformed query set: {path}: case {id_repr} 'relevant' must be a non-empty list"
        ));
    };
    if relevant.is_empty() {
        return usage(format!(
            "malformed query set: {path}: case {id_repr} 'relevant' must be a non-empty list"
        ));
    }
    let must_not = map.get("must_not_return").cloned().unwrap_or(Value::Array(Vec::new()));
    let Value::Array(must_not) = must_not else {
        return usage(format!(
            "malformed query set: {path}: case {id_repr} 'must_not_return' must be a list"
        ));
    };
    let artifact_type = match map.get("type") {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) => Some(s.clone()),
        Some(_) => {
            return usage(format!(
                "malformed query set: {path}: case {id_repr} 'type' must be a string"
            ))
        }
    };
    Ok(QueryCase {
        id: py_str(&case_id),
        tool: py_str(&tool),
        query: py_str(&query),
        category: py_str(&category),
        relevant: relevant.iter().map(py_str).collect(),
        must_not_return: must_not.iter().map(py_str).collect(),
        artifact_type,
    })
}

/// `load_baseline(path)` — the committed baseline `metrics` object.
pub fn load_baseline(path: &str) -> EvalResult<Value> {
    let data = load_json(path, "baseline")?;
    match &data {
        Value::Object(map) if map.contains_key("overall") => Ok(data),
        _ => usage(format!("malformed baseline: {path}: expected a metrics object")),
    }
}

/// `load_config(path)` — floors and tolerance.
pub fn load_config(path: &str) -> EvalResult<Value> {
    let data = load_json(path, "config")?;
    match &data {
        Value::Object(map) if map.contains_key("floors") && map.contains_key("tolerance") => {
            Ok(data)
        }
        _ => usage(format!("malformed config: {path}: expected 'floors' and 'tolerance'")),
    }
}

// --- Retrieval seam: the real surface, never a parallel scorer (REQ-002) -----

/// Rank of a snake_case relationship section in the canonical order
/// (`_RELATIONSHIP_ORDER`); unknown sections rank last. Mirrors
/// `decided-mcp::graph::relationship_order`.
fn relationship_order(section: &str) -> usize {
    for (i, (name, _)) in RELATIONSHIP_SECTIONS.iter().enumerate() {
        if snake(name) == section {
            return i;
        }
    }
    RELATIONSHIP_SECTIONS.len()
}

/// The ordered incoming-reference id list for `target_path` — exactly the
/// `incoming` order the MCP `get_related` tool returns (mirrors
/// `decided-mcp::graph::incoming_references`, `MAX_RELATED_EDGES` = 1000).
fn incoming_ids(
    relationships: &[Relationship],
    identity_by_path: &HashMap<&str, &str>,
    target_path: &str,
) -> Vec<String> {
    const MAX_RELATED_EDGES: usize = 1000;
    let mut incoming: Vec<(usize, String, String)> = Vec::new(); // (rank, id, path)
    for rel in relationships {
        if rel.resolved_path.as_deref() != Some(target_path) {
            continue;
        }
        if rel.source_path == target_path {
            continue; // self-references are not incoming edges
        }
        let Some(&id) = identity_by_path.get(rel.source_path.as_str()) else {
            continue;
        };
        if incoming.len() < MAX_RELATED_EDGES {
            incoming.push((
                relationship_order(&rel.relationship),
                id.to_string(),
                rel.source_path.clone(),
            ));
        }
    }
    incoming.sort_by(|a, b| (a.0, &a.1, &a.2).cmp(&(b.0, &b.1, &b.2)));
    incoming.into_iter().map(|(_, id, _)| id).collect()
}

/// Returned ids for a `search_artifacts` case: `search_index` order verbatim.
fn search_returned(entries: &[IndexEntry], case: &QueryCase) -> Vec<String> {
    let result = search_index(entries, &case.query, case.artifact_type.as_deref(), &[]);
    result.matches.into_iter().map(|m| m.id).collect()
}

/// Returned ids for a `get_related` case — the tool's `incoming` order.
/// A query that does not resolve is a malformed case (usage error).
fn related_returned(root: &str, case: &QueryCase) -> EvalResult<Vec<String>> {
    let corpus = corpus_items(root, true);
    let index = index_from_items(&corpus);
    let resolution = resolve_in_index(&index, &case.query);
    let Some(artifact) = resolution
        .artifact
        .as_ref()
        .filter(|_| resolution.outcome == OUTCOME_RESOLVED)
    else {
        return usage(format!(
            "get_related case {}: query {} did not resolve to an artifact in {}",
            py_repr_str(&case.id),
            py_repr_str(&case.query),
            py_repr_str(root)
        ));
    };
    let relationships = relationships_from_corpus(&corpus);
    let identity_by_path: HashMap<&str, &str> =
        index.iter().map(|e| (e.path.as_str(), e.id.as_str())).collect();
    Ok(incoming_ids(&relationships, &identity_by_path, &artifact.path))
}

fn related_returned_composed(
    corpus: &ComposedCorpus,
    root: &str,
    case: &QueryCase,
) -> EvalResult<Vec<String>> {
    const MAX_RELATED_EDGES: usize = 1000;
    let artifact = corpus.resolve(&case.query).map_err(|_| {
        EvalUsageError(format!(
            "get_related case {}: query {} did not resolve to an artifact in {}",
            py_repr_str(&case.id),
            py_repr_str(&case.query),
            py_repr_str(root)
        ))
    })?;
    let relationships = corpus.relationships();
    let identity_by_path: HashMap<&ArtifactPath, &str> = corpus
        .effective()
        .map(|item| (&item.artifact_path, item.key.canonical_id.as_str()))
        .collect();
    let mut incoming = Vec::new();
    for relationship in &relationships {
        if relationship.resolved_artifact.as_ref() != Some(&artifact.artifact_path)
            || relationship.source_artifact.as_ref() == Some(&artifact.artifact_path)
        {
            continue;
        }
        let Some(source_path) = relationship.source_artifact.as_ref() else {
            continue;
        };
        let Some(id) = identity_by_path.get(source_path) else {
            continue;
        };
        if incoming.len() < MAX_RELATED_EDGES {
            incoming.push((
                relationship_order(&relationship.relationship),
                (*id).to_string(),
                source_path.clone(),
            ));
        }
    }
    incoming.sort_by(|left, right| (left.0, &left.1, &left.2).cmp(&(right.0, &right.1, &right.2)));
    Ok(incoming.into_iter().map(|(_, id, _)| id).collect())
}

fn returned_ids(
    root: &str,
    entries: &[IndexEntry],
    composed: Option<&ComposedCorpus>,
    case: &QueryCase,
) -> EvalResult<Vec<String>> {
    if case.tool == TOOL_SEARCH {
        Ok(search_returned(entries, case))
    } else if let Some(corpus) = composed {
        related_returned_composed(corpus, root, case)
    } else {
        related_returned(root, case)
    }
}

// --- Per-case scoring ---------------------------------------------------------

/// `score_case(returned, case)` — P@k, R@k, hard-negative violations.
fn score_case(returned: Vec<String>, case: QueryCase) -> CaseResult {
    let relevant: std::collections::HashSet<&str> =
        case.relevant.iter().map(String::as_str).collect();
    let mut precision = [0.0f64; 3];
    let mut recall = [0.0f64; 3];
    for (i, &k) in K_VALUES.iter().enumerate() {
        let top_k = &returned[..k.min(returned.len())];
        let hits = top_k.iter().filter(|rid| relevant.contains(rid.as_str())).count();
        precision[i] = hits as f64 / k as f64;
        recall[i] = hits as f64 / case.relevant.len() as f64;
    }
    let negatives: std::collections::HashSet<&str> =
        case.must_not_return.iter().map(String::as_str).collect();
    let mut violations: Vec<String> = returned
        .iter()
        .take(NEGATIVE_K)
        .filter(|rid| negatives.contains(rid.as_str()))
        .cloned()
        .collect();
    violations.sort();
    CaseResult {
        case,
        returned,
        precision,
        recall,
        violations,
    }
}

// --- Aggregation ----------------------------------------------------------------

fn mean(values: &[f64]) -> f64 {
    if values.is_empty() {
        0.0
    } else {
        values.iter().sum::<f64>() / values.len() as f64
    }
}

fn overall_metrics(results: &[CaseResult]) -> Value {
    let mut m = Map::new();
    for (i, k) in K_VALUES.iter().enumerate() {
        let values: Vec<f64> = results.iter().map(|r| r.precision[i]).collect();
        m.insert(format!("p_at_{k}"), py_float(round6(mean(&values))));
    }
    for (i, k) in K_VALUES.iter().enumerate() {
        let values: Vec<f64> = results.iter().map(|r| r.recall[i]).collect();
        m.insert(format!("r_at_{k}"), py_float(round6(mean(&values))));
    }
    let negatives: i64 = results.iter().map(|r| r.violations.len() as i64).sum();
    m.insert("negative_violations".into(), Value::from(negatives));
    Value::Object(m)
}

/// `{group -> {p_at_1, r_at_5}}` macro-averaged within each group, sorted.
fn grouped_metrics(results: &[CaseResult], key: impl Fn(&CaseResult) -> &str) -> Value {
    let mut groups: Vec<(&str, Vec<&CaseResult>)> = Vec::new();
    for result in results {
        let name = key(result);
        match groups.iter_mut().find(|(n, _)| *n == name) {
            Some((_, members)) => members.push(result),
            None => groups.push((name, vec![result])),
        }
    }
    groups.sort_by(|a, b| a.0.cmp(b.0));
    let mut out = Map::new();
    for (name, members) in groups {
        let p1: Vec<f64> = members.iter().map(|r| r.precision[0]).collect();
        let r5: Vec<f64> = members.iter().map(|r| r.recall[2]).collect();
        let mut cell = Map::new();
        cell.insert("p_at_1".into(), py_float(round6(mean(&p1))));
        cell.insert("r_at_5".into(), py_float(round6(mean(&r5))));
        out.insert(name.to_string(), Value::Object(cell));
    }
    Value::Object(out)
}

// --- Hashing the inputs (diagnostic metadata, excluded from the gate) --------

/// `corpus_hash(root)` — `sha256:` over rel-path + NUL + bytes + NUL per
/// walked Markdown file, in the corpus walk's sorted order (REQ-005).
pub fn corpus_hash(root: &str) -> String {
    let mut digest = Sha256::new();
    for entry in find_markdown_files(root, true) {
        digest.update(entry.rel().as_bytes());
        digest.update(b"\0");
        digest.update(&std::fs::read(&entry.abs).unwrap_or_default());
        digest.update(b"\0");
    }
    format!("sha256:{}", digest.hexdigest())
}

fn composed_corpus_hash(corpus: &ComposedCorpus) -> String {
    let mut digest = Sha256::new();
    for item in corpus.catalog() {
        digest.update(item.artifact_path.source.as_bytes());
        digest.update(b"\0");
        digest.update(item.artifact_path.relative_path.as_bytes());
        digest.update(b"\0");
        digest.update(corpus.content(&item.key).unwrap_or_default());
        digest.update(b"\0");
    }
    format!("sha256:{}", digest.hexdigest())
}

/// `query_set_hash(path)` — `sha256:` over the raw file bytes.
pub fn query_set_hash(path: &str) -> String {
    format!(
        "sha256:{}",
        crate::sha256::hexdigest(&std::fs::read(path).unwrap_or_default())
    )
}

// --- Top-level run ------------------------------------------------------------

/// `run_eval(root, queries_path)` (REQ-001..REQ-005).
pub fn run_eval(root: &str, queries_path: &str) -> EvalResult<Scorecard> {
    if !Path::new(root).is_dir() {
        return usage(format!("corpus not found or not a directory: {root}"));
    }
    let cases = load_query_set(queries_path)?;
    let composed = crate::federated_corpus::load_composed_corpus(root, true)
        .map_err(|error| EvalUsageError(error.to_string()))?;
    let entries = composed
        .as_ref()
        .map(ComposedCorpus::effective_index)
        .unwrap_or_else(|| build_index(root, true));

    let mut results: Vec<CaseResult> = Vec::with_capacity(cases.len());
    for case in cases {
        let returned = returned_ids(root, &entries, composed.as_ref(), &case)?;
        results.push(score_case(returned, case));
    }
    let n_queries = results.len() as i64;
    results.sort_by(|a, b| a.case.id.cmp(&b.case.id));

    let mut metrics = Map::new();
    metrics.insert("overall".into(), overall_metrics(&results));
    metrics.insert(
        "by_category".into(),
        grouped_metrics(&results, |r| r.case.category.as_str()),
    );
    metrics.insert("by_tool".into(), grouped_metrics(&results, |r| r.case.tool.as_str()));

    let mut metadata = Map::new();
    metadata.insert(
        "lore_version".into(),
        Value::String(crate::output::rac_version()),
    );
    metadata.insert(
        "corpus_hash".into(),
        Value::String(
            composed
                .as_ref()
                .map(composed_corpus_hash)
                .unwrap_or_else(|| corpus_hash(root)),
        ),
    );
    metadata.insert(
        "query_set_hash".into(),
        Value::String(query_set_hash(queries_path)),
    );
    metadata.insert("n_queries".into(), Value::from(n_queries));
    metadata.insert("generated_at".into(), Value::String(now_iso()));

    let per_query: Vec<Value> = results.iter().map(CaseResult::to_value).collect();
    Ok(Scorecard {
        metrics: Value::Object(metrics),
        metadata: Value::Object(metadata),
        per_query,
    })
}

/// `datetime.now(UTC).isoformat()` — diagnostic metadata only.
fn now_iso() -> String {
    let (secs, micros) = crate::consent::now_epoch();
    crate::consent::utc_isoformat_micros(secs, micros)
}

// --- The gate (`decided eval --check`) --------------------------------------------

const RULE_NEGATIVE: &str = "negative_violations";
const RULE_FLOOR: &str = "floor";
const RULE_REGRESSION: &str = "regression";

/// One fired gate rule.
pub struct GateFailure {
    rule: &'static str,
    metric: String,
    threshold: f64,
    current: f64,
}

impl GateFailure {
    /// `GateFailure.render()` — the byte-refereed stdout lines.
    pub fn render(&self) -> String {
        use crate::pycompat::py_format_fixed;
        if self.rule == RULE_NEGATIVE {
            return format!(
                "FAIL [negative_violations] {}: limit {}, current {}",
                self.metric,
                py_format_fixed(self.threshold, 0),
                py_format_fixed(self.current, 0)
            );
        }
        let label = if self.rule == RULE_FLOOR { "floor" } else { "baseline" };
        format!(
            "FAIL [{}] {}: {} {}, current {}",
            self.rule,
            self.metric,
            label,
            py_format_fixed(self.threshold, 6),
            py_format_fixed(self.current, 6)
        )
    }
}

/// `float(value)` over a gate-config JSON scalar.
fn as_float(value: &Value) -> Option<f64> {
    value.as_f64()
}

/// The `(scope, name, metric)` triples the gate enforces beyond negatives:
/// ONLY `p_at_1` and `r_at_5`, and only where a floor is declared (a floor
/// on any other metric is silently ignored — eval brief, landmine 2).
fn gated_pairs(config: &Value) -> Vec<(String, String, String)> {
    let mut pairs = Vec::new();
    let floors = &config["floors"];
    for metric in ["p_at_1", "r_at_5"] {
        if floors
            .get("overall")
            .and_then(|o| o.get(metric))
            .is_some()
        {
            pairs.push(("overall".to_string(), String::new(), metric.to_string()));
        }
    }
    if let Some(Value::Object(by_category)) = floors.get("by_category") {
        let mut categories: Vec<&String> = by_category.keys().collect();
        categories.sort();
        for category in categories {
            for metric in ["p_at_1", "r_at_5"] {
                if by_category[category].get(metric).is_some() {
                    pairs.push((
                        "by_category".to_string(),
                        category.clone(),
                        metric.to_string(),
                    ));
                }
            }
        }
    }
    pairs
}

fn metric_value(metrics: &Value, scope: &str, name: &str, metric: &str) -> Option<f64> {
    let block = metrics.get(scope)?;
    let value = if scope == "overall" {
        block.get(metric)
    } else {
        block.get(name)?.get(metric)
    };
    value.and_then(as_float)
}

fn floor_value(floors: &Value, scope: &str, name: &str, metric: &str) -> Option<f64> {
    let value = if scope == "overall" {
        floors.get("overall")?.get(metric)
    } else {
        floors.get(scope)?.get(name)?.get(metric)
    };
    value.and_then(as_float)
}

/// `evaluate_gate(current, baseline, config)` (REQ-006) — one failure per
/// fired rule, deterministic order: negatives, then per gated pair
/// (missing-metric floor / floor / regression).
pub fn evaluate_gate(current: &Value, baseline: &Value, config: &Value) -> Vec<GateFailure> {
    let mut failures: Vec<GateFailure> = Vec::new();
    let tolerance = config
        .get("tolerance")
        .and_then(as_float)
        .unwrap_or(0.0);
    let floors = &config["floors"];

    // (a) Hard-negative violations — always gated.
    // `int(x)` truncates a float-typed JSON count.
    let as_int = |v: &Value| v.as_i64().or_else(|| v.as_f64().map(|f| f as i64));
    let negatives = current
        .get("overall")
        .and_then(|o| o.get("negative_violations"))
        .and_then(as_int)
        .unwrap_or(0);
    let negatives_max = floors
        .get("negative_violations")
        .and_then(as_int)
        .unwrap_or(0);
    if negatives > negatives_max {
        failures.push(GateFailure {
            rule: RULE_NEGATIVE,
            metric: "overall.negative_violations".to_string(),
            threshold: negatives_max as f64,
            current: negatives as f64,
        });
    }

    for (scope, name, metric) in gated_pairs(config) {
        let dotted = if name.is_empty() {
            format!("{scope}.{metric}")
        } else {
            format!("{scope}.{name}.{metric}")
        };
        let value = metric_value(current, &scope, &name, &metric);
        let floor = floor_value(floors, &scope, &name, &metric);
        let Some(value) = value else {
            // A gated metric absent from the current run is a regression.
            failures.push(GateFailure {
                rule: RULE_FLOOR,
                metric: dotted,
                threshold: floor.unwrap_or(0.0),
                current: 0.0,
            });
            continue;
        };
        if let Some(floor) = floor {
            if value < floor {
                failures.push(GateFailure {
                    rule: RULE_FLOOR,
                    metric: dotted.clone(),
                    threshold: floor,
                    current: value,
                });
            }
        }
        if let Some(base) = metric_value(baseline, &scope, &name, &metric) {
            if value < base - tolerance {
                failures.push(GateFailure {
                    rule: RULE_REGRESSION,
                    metric: dotted,
                    threshold: base,
                    current: value,
                });
            }
        }
    }
    failures
}

#[cfg(test)]
#[allow(clippy::items_after_test_module)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn gate_only_enforces_p1_and_r5_where_floored() {
        let config = json!({
            "tolerance": 0.02,
            "floors": {"overall": {"p_at_1": 0.9, "p_at_5": 0.99}}
        });
        // p_at_5 floor is silently ignored (landmine 2).
        assert_eq!(
            gated_pairs(&config),
            vec![("overall".to_string(), String::new(), "p_at_1".to_string())]
        );
    }

    #[test]
    fn gate_failure_render_shapes() {
        let neg = GateFailure {
            rule: RULE_NEGATIVE,
            metric: "overall.negative_violations".into(),
            threshold: -1.0,
            current: 0.0,
        };
        assert_eq!(
            neg.render(),
            "FAIL [negative_violations] overall.negative_violations: limit -1, current 0"
        );
        let floor = GateFailure {
            rule: RULE_FLOOR,
            metric: "overall.p_at_1".into(),
            threshold: 1.5,
            current: 1.0,
        };
        assert_eq!(
            floor.render(),
            "FAIL [floor] overall.p_at_1: floor 1.500000, current 1.000000"
        );
        let reg = GateFailure {
            rule: RULE_REGRESSION,
            metric: "overall.p_at_1".into(),
            threshold: 1.5,
            current: 1.0,
        };
        assert_eq!(
            reg.render(),
            "FAIL [regression] overall.p_at_1: baseline 1.500000, current 1.000000"
        );
    }

    #[test]
    fn score_case_windows() {
        let case = QueryCase {
            id: "T1".into(),
            tool: "search_artifacts".into(),
            query: "q".into(),
            category: "c".into(),
            relevant: vec!["A".into(), "B".into()],
            must_not_return: vec!["X".into()],
            artifact_type: None,
        };
        let result = score_case(
            vec!["A".into(), "X".into(), "B".into()],
            case,
        );
        assert_eq!(result.precision, [1.0, 2.0 / 3.0, 2.0 / 5.0]);
        assert_eq!(result.recall, [0.5, 1.0, 1.0]);
        assert_eq!(result.violations, vec!["X".to_string()]);
    }
}

// --- Rendering (module-local: the scorecard shapes live here) ------------------

/// `render_scorecard_json(scorecard)` — pretty JSON, `ensure_ascii=False`.
pub fn render_scorecard_json(scorecard: &Scorecard) -> String {
    crate::pyjson::dumps_indent2_no_ascii(&scorecard.to_value())
}

/// `render_metrics_json(metrics)` — what `--update-baseline` writes.
pub fn render_metrics_json(metrics: &Value) -> String {
    crate::pyjson::dumps_indent2_no_ascii(metrics)
}

/// `render_scorecard_human(scorecard)` — overall / by-category / by-tool /
/// Violations, Python format-spec faithful.
pub fn render_scorecard_human(scorecard: &Scorecard) -> String {
    use crate::pycompat::py_format_fixed;
    let rjust = |s: &str, w: usize| -> String {
        let n = s.chars().count();
        if n >= w {
            s.to_string()
        } else {
            format!("{}{}", " ".repeat(w - n), s)
        }
    };
    let f = |v: f64, w: usize, nd: usize| rjust(&py_format_fixed(v, nd), w);
    let get = |obj: &Value, key: &str| obj.get(key).and_then(Value::as_f64).unwrap_or(0.0);

    let metrics = &scorecard.metrics;
    let mut lines: Vec<String> = Vec::new();

    let overall = &metrics["overall"];
    lines.push("Overall".to_string());
    let mut header = "  ".to_string();
    for k in K_VALUES {
        header.push_str(&rjust(&format!("P@{k}"), 8));
        header.push_str(&rjust(&format!("R@{k}"), 8));
    }
    lines.push(header);
    let mut row = "  ".to_string();
    for k in K_VALUES {
        row.push_str(&f(get(overall, &format!("p_at_{k}")), 8, 3));
        row.push_str(&f(get(overall, &format!("r_at_{k}")), 8, 3));
    }
    lines.push(row);
    lines.push(format!(
        "  negative_violations: {}",
        overall
            .get("negative_violations")
            .and_then(Value::as_i64)
            .unwrap_or(0)
    ));
    lines.push(String::new());

    lines.push("By category".to_string());
    render_group(&metrics["by_category"], &mut lines);
    lines.push(String::new());

    lines.push("By tool".to_string());
    render_group(&metrics["by_tool"], &mut lines);
    lines.push(String::new());

    lines.push("Violations".to_string());
    let offenders: Vec<&Value> = scorecard
        .per_query
        .iter()
        .filter(|entry| {
            entry
                .get("violations")
                .and_then(Value::as_array)
                .is_some_and(|v| !v.is_empty())
        })
        .collect();
    if offenders.is_empty() {
        lines.push("  none".to_string());
    } else {
        for offender in offenders {
            let id = offender.get("id").and_then(Value::as_str).unwrap_or("");
            let tool = offender.get("tool").and_then(Value::as_str).unwrap_or("");
            let violations = py_repr_str_list(offender.get("violations"));
            let returned = py_repr_str_list(offender.get("returned"));
            lines.push(format!(
                "  {id} ({tool}): returned {violations} in top-{NEGATIVE_K} [returned={returned}]"
            ));
        }
    }
    lines.join("\n")
}

/// `repr(list[str])` — `['a', 'b']`, elements via CPython `repr(str)`.
fn py_repr_str_list(value: Option<&Value>) -> String {
    let items: Vec<String> = value
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .map(|v| py_repr_str(v.as_str().unwrap_or("")))
                .collect()
        })
        .unwrap_or_default();
    format!("[{}]", items.join(", "))
}

fn render_group(group: &Value, lines: &mut Vec<String>) {
    use crate::pycompat::py_format_fixed;
    let Some(map) = group.as_object() else {
        lines.push("  (none)".to_string());
        return;
    };
    if map.is_empty() {
        lines.push("  (none)".to_string());
        return;
    }
    let width = map.keys().map(|name| name.chars().count()).max().unwrap_or(0);
    lines.push(format!("  {}    P@1     R@5", " ".repeat(width)));
    for (name, cell) in map {
        let pad = width.saturating_sub(name.chars().count());
        let p1 = cell.get("p_at_1").and_then(Value::as_f64).unwrap_or(0.0);
        let r5 = cell.get("r_at_5").and_then(Value::as_f64).unwrap_or(0.0);
        let p1s = py_format_fixed(p1, 3);
        let r5s = py_format_fixed(r5, 3);
        lines.push(format!(
            "  {}{}  {:>6}  {:>6}",
            name,
            " ".repeat(pad),
            p1s,
            r5s
        ));
    }
}
