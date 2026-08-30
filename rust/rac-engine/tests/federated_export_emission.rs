use std::fs;
use std::path::{Path, PathBuf};

use rac_engine::composition::{
    ArtifactOverrideProvenance, ComposedCorpus, ComposedProvenance, OverrideDeclaration,
    OverrideRole, ParentIdentity,
};
use rac_engine::corpus::{
    ArtifactKey, CorpusLayer, Layer, PhysicalArtifactLocator, PhysicalCorpusLocator,
};
use rac_engine::export::{
    build_corpus_export, build_corpus_export_from_composed, build_documents_export,
    build_documents_export_from_composed, build_graph_export, build_graph_export_from_composed,
    build_okf_export_from_composed, export_schema, CorpusExport, DocumentsExport, ExportArtifact,
    ExportDocument, ExportIdentity, ExportRelationship, GraphEdge, GraphExport, GraphNode,
};
use rac_engine::graph_composition::{
    GraphComposition, GraphCompositionInput, GraphOverrideDeclaration, SourceNodeInput,
    SourceParentInput,
};
use rac_engine::output::{render_documents_jsonl, render_export_json, render_graph_json};
use rac_engine::parse::parse_text;
use rac_engine::portal::{render_export_html, render_federated_export_html, FEDERATED_SHELL, SHELL};
use rac_engine::relationships::{corpus_items, CorpusItem};
use rac_engine::spec::spec_for;
use serde_json::Value;

const PIN: &str = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const GRAPH_PIN: &str =
    "sha256-v2:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

const PARENT_POLICY: &str = r#"---
schema_version: 1
id: STD-001
type: decision
---
# ADR-001: Parent Policy

## ID

STD-001

## Status

Accepted

## Context

The parent sets a policy.

## Decision

Use the shared policy.

## Consequences

Children inherit it.

## Related Decisions

- STD-002
"#;

const PARENT_RELATED: &str = r#"---
schema_version: 1
id: STD-002
type: decision
---
# ADR-002: Related Parent Policy

## ID

STD-002

## Status

Accepted

## Context

The relationship needs a target.

## Decision

Retain source-aware endpoints.

## Consequences

Graph exports remain unambiguous.
"#;

const LOCAL_REPLACEMENT: &str = r#"---
schema_version: 1
id: APP-001
type: decision
---
# ADR-001: Local Replacement

## ID

APP-001

## Status

Accepted

## Context

The child needs a bounded exception.

## Decision

Replace the parent policy locally.

## Consequences

The parent remains auditable.
"#;

const LOCAL_RATIONALE: &str = r#"---
schema_version: 1
id: APP-ADR-001
type: decision
---
# ADR-002: Explain the Override

## ID

APP-ADR-001

## Status

Accepted

## Context

Overrides require a live decision.

## Decision

Approve the local exception.

## Consequences

The mapping has durable provenance.
"#;

fn identity(source: &str, id: &str) -> ExportIdentity {
    ExportIdentity {
        source: source.to_string(),
        id: id.to_string(),
    }
}

fn key(source: &str, id: &str) -> ArtifactKey {
    ArtifactKey::new(source, id)
}

fn scratch(tag: &str) -> PathBuf {
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "asdecided-export-{tag}-{}-{unique}",
        std::process::id()
    ));
    fs::create_dir_all(root.join("child/decisions")).unwrap();
    fs::create_dir_all(root.join("parent/decisions")).unwrap();
    fs::create_dir_all(root.join("child/.decided")).unwrap();
    fs::create_dir_all(root.join("parent/.decided")).unwrap();
    fs::write(
        root.join("child/.decided/config.yaml"),
        "repository_key: APP\ncorpus:\n  source: acme/app\n",
    )
    .unwrap();
    fs::write(
        root.join("parent/.decided/config.yaml"),
        "repository_key: STD\ncorpus:\n  source: acme/standards\n",
    )
    .unwrap();
    fs::write(
        root.join("child/decisions/replacement.md"),
        LOCAL_REPLACEMENT,
    )
    .unwrap();
    fs::write(root.join("child/decisions/rationale.md"), LOCAL_RATIONALE).unwrap();
    fs::write(root.join("parent/decisions/policy.md"), PARENT_POLICY).unwrap();
    fs::write(root.join("parent/decisions/related.md"), PARENT_RELATED).unwrap();
    root
}

fn corpus_path(root: &Path, layer: &str) -> String {
    root.join(layer)
        .join("decisions")
        .to_string_lossy()
        .into_owned()
}

fn inherited_items(parent_directory: &str) -> Vec<CorpusItem> {
    let origin = CorpusLayer::inherited("acme/standards", "standards", PIN).origin();
    corpus_items(parent_directory, true)
        .into_iter()
        .map(|mut item| {
            item.key.source = origin.source.clone();
            item.artifact_path.source = origin.source.clone();
            item.origin = origin.clone();
            item
        })
        .collect()
}

fn composed_fixture(root: &Path) -> ComposedCorpus {
    let child_directory = corpus_path(root, "child");
    let parent_directory = corpus_path(root, "parent");
    let local = corpus_items(&child_directory, true);
    let inherited = inherited_items(&parent_directory);
    let content: Vec<_> = local
        .iter()
        .chain(&inherited)
        .map(|item| (item.key.clone(), fs::read(&item.locator.path).unwrap()))
        .collect();
    ComposedCorpus::compose_with_content(
        local,
        inherited,
        ParentIdentity::new("acme/standards", "standards").unwrap(),
        vec![OverrideDeclaration::parse("standards::STD-001", "APP-001", "APP-ADR-001").unwrap()],
        content,
    )
}

fn overridden_provenance() -> ComposedProvenance {
    ComposedProvenance {
        origin: CorpusLayer::inherited("acme/standards", "standards", PIN).origin(),
        overrides: vec![ArtifactOverrideProvenance {
            state: OverrideRole::Overridden,
            parent: key("acme/standards", "STD-001"),
            replacement: key("acme/app", "APP-001"),
            rationale: key("acme/app", "APP-ADR-001"),
        }],
    }
}

fn graph_item(
    source: &str,
    relative_path: &str,
    id: &str,
    artifact_type: &str,
    relationships: &str,
) -> (CorpusItem, Vec<u8>) {
    let body = match artifact_type {
        "decision" => {
            "## Context\n\nExport fixture.\n\n## Decision\n\nRetain complete provenance.\n\n## Consequences\n\nPolicy history stays auditable.\n"
        }
        "requirement" => {
            "## Problem\n\nGraph exports need stable identity.\n\n## Requirements\n\n- [REQ-001] Exports MUST retain source-aware history.\n"
        }
        other => panic!("unsupported fixture type {other}"),
    };
    let text = format!(
        "---\nschema_version: 1\ntype: {artifact_type}\n---\n# {id}\n\n## ID\n\n{id}\n\n## Status\n\nAccepted\n\n{body}\n{relationships}"
    );
    let origin = if source == "acme/app" {
        CorpusLayer::local(source).origin()
    } else {
        CorpusLayer::inherited(source, "runtime-only", GRAPH_PIN).origin()
    };
    let display = format!("/runtime/{source}/{relative_path}");
    let item = CorpusItem::new(
        display.clone(),
        relative_path.to_string(),
        parse_text(&text, &display),
        spec_for(artifact_type),
        origin,
        PhysicalArtifactLocator::new(
            PhysicalCorpusLocator::new(
                format!("/runtime/{source}"),
                format!("/runtime/{source}/decisions"),
            ),
            display,
        ),
    );
    (item, text.into_bytes())
}

fn graph_corpus(
    nodes: Vec<SourceNodeInput>,
    fixtures: Vec<(CorpusItem, Vec<u8>)>,
    overrides: Vec<GraphOverrideDeclaration>,
) -> ComposedCorpus {
    let (items, contents): (Vec<_>, Vec<_>) = fixtures
        .into_iter()
        .map(|(item, bytes)| {
            let key = item.key.clone();
            (item, (key, bytes))
        })
        .unzip();
    let graph = GraphComposition::compose(GraphCompositionInput::new(
        "acme/app",
        nodes,
        items,
        overrides,
    ))
    .unwrap();
    ComposedCorpus::from_graph(graph, contents, Vec::new())
}

fn graph_parent(source: &str, alias: &str) -> SourceParentInput {
    SourceParentInput::new(source, alias)
}

fn graph_node(source: &str, parents: Vec<SourceParentInput>) -> SourceNodeInput {
    SourceNodeInput::new(source, parents)
}

fn graph_mapping(
    owner: &str,
    target_source: &str,
    target: &str,
    replacement: &str,
    rationale: &str,
) -> GraphOverrideDeclaration {
    GraphOverrideDeclaration::new(
        owner,
        key(target_source, target),
        key(owner, replacement),
        key(owner, rationale),
    )
}

fn diamond_chain_corpus() -> ComposedCorpus {
    let shared = "acme/shared";
    let left = "acme/left";
    let right = "acme/right";
    graph_corpus(
        vec![
            graph_node(
                "acme/app",
                vec![graph_parent(left, "left"), graph_parent(right, "right")],
            ),
            graph_node(left, vec![graph_parent(shared, "base")]),
            graph_node(right, vec![graph_parent(shared, "base")]),
            graph_node(shared, Vec::new()),
        ],
        vec![
            graph_item(shared, "policy.md", "POLICY", "requirement", ""),
            graph_item(
                shared,
                "consumer.md",
                "CONSUMER",
                "requirement",
                "## Related Requirements\n\n- POLICY\n",
            ),
            graph_item(left, "replacement.md", "LEFT-POLICY", "requirement", ""),
            graph_item(left, "rationale.md", "LEFT-ADR", "decision", ""),
            graph_item(
                "acme/app",
                "replacement.md",
                "ROOT-POLICY",
                "requirement",
                "",
            ),
            graph_item("acme/app", "rationale.md", "ROOT-ADR", "decision", ""),
        ],
        vec![
            graph_mapping(left, shared, "POLICY", "LEFT-POLICY", "LEFT-ADR"),
            graph_mapping(
                "acme/app",
                left,
                "LEFT-POLICY",
                "ROOT-POLICY",
                "ROOT-ADR",
            ),
            graph_mapping(
                "acme/app",
                shared,
                "POLICY",
                "ROOT-POLICY",
                "ROOT-ADR",
            ),
        ],
    )
}

#[test]
fn viewer_emits_record_edge_endpoint_and_override_provenance() {
    let provenance = overridden_provenance();
    let export = CorpusExport {
        corpus_name: "decisions".to_string(),
        corpus_source: "acme/app".to_string(),
        rac_version: "test".to_string(),
        artifacts: vec![ExportArtifact {
            id: "STD-001".to_string(),
            aliases: vec!["parent-policy".to_string()],
            artifact_type: "decision".to_string(),
            status: "Accepted".to_string(),
            title: "Parent policy".to_string(),
            path: "decisions/parent-policy.md".to_string(),
            body_html: "<p>Policy.</p>".to_string(),
            tags: Vec::new(),
            provenance: Some(provenance.clone()),
            graph_provenance: None,
        }],
        relationships: vec![ExportRelationship {
            from: "STD-001".to_string(),
            to: "APP-001".to_string(),
            edge_type: "relates-to".to_string(),
            from_identity: Some(identity("acme/standards", "STD-001")),
            to_identity: Some(identity("acme/app", "APP-001")),
            provenance: Some(provenance),
            graph_provenance: None,
            authored_token: None,
            historical_candidates: Vec::new(),
            effective_terminal: None,
        }],
    };

    let value: Value = serde_json::from_str(&render_export_json(&export)).unwrap();
    let artifact = &value["artifacts"][0];
    assert_eq!(artifact["provenance"]["source"], "acme/standards");
    assert_eq!(artifact["provenance"]["layer"], "inherited");
    assert_eq!(artifact["provenance"]["pin"], PIN);
    assert_eq!(
        artifact["provenance"]["overrides"][0]["state"],
        "overridden"
    );
    assert_eq!(
        artifact["provenance"]["overrides"][0]["replacement"],
        serde_json::json!({"source": "acme/app", "id": "APP-001"})
    );

    let relationship = &value["relationships"][0];
    assert_eq!(
        relationship["from_identity"],
        serde_json::json!({"source": "acme/standards", "id": "STD-001"})
    );
    assert_eq!(
        relationship["to_identity"],
        serde_json::json!({"source": "acme/app", "id": "APP-001"})
    );
    assert_eq!(relationship["provenance"]["pin"], PIN);
}

#[test]
fn documents_and_graph_emit_the_same_per_record_provenance() {
    let provenance = overridden_provenance();
    let documents = DocumentsExport {
        corpus_name: "decisions".to_string(),
        corpus_source: "acme/app".to_string(),
        documents: vec![ExportDocument {
            id: "STD-001".to_string(),
            artifact_type: "decision".to_string(),
            status: "Accepted".to_string(),
            title: "Parent policy".to_string(),
            text: "# Parent policy\n".to_string(),
            aliases: Vec::new(),
            path: "decisions/parent-policy.md".to_string(),
            tags: Vec::new(),
            provenance: Some(provenance.clone()),
            graph_provenance: None,
        }],
    };
    let document: Value = serde_json::from_str(&render_documents_jsonl(&documents)).unwrap();
    assert_eq!(document["metadata"]["source"], "acme/standards");
    assert_eq!(
        document["metadata"]["provenance"]["source"],
        "acme/standards"
    );
    assert_eq!(document["metadata"]["provenance"]["layer"], "inherited");
    assert_eq!(document["metadata"]["provenance"]["pin"], PIN);

    let graph = GraphExport {
        corpus_name: "decisions".to_string(),
        corpus_source: "acme/app".to_string(),
        nodes: vec![GraphNode {
            id: "STD-001".to_string(),
            artifact_type: "decision".to_string(),
            status: "Accepted".to_string(),
            title: "Parent policy".to_string(),
            provenance: Some(provenance.clone()),
            graph_provenance: None,
        }],
        edges: vec![GraphEdge {
            source: "STD-001".to_string(),
            target: "APP-001".to_string(),
            edge_type: "related_decisions".to_string(),
            directed: false,
            resolved: true,
            external: false,
            provider: None,
            source_identity: Some(identity("acme/standards", "STD-001")),
            target_identity: Some(identity("acme/app", "APP-001")),
            provenance: Some(provenance),
            graph_provenance: None,
            authored_token: None,
            historical_candidates: Vec::new(),
            effective_terminal: None,
        }],
    };
    let value: Value = serde_json::from_str(&render_graph_json(&graph)).unwrap();
    assert_eq!(value["nodes"][0]["provenance"]["source"], "acme/standards");
    assert_eq!(value["nodes"][0]["provenance"]["pin"], PIN);
    assert_eq!(
        value["edges"][0]["source_identity"],
        serde_json::json!({"source": "acme/standards", "id": "STD-001"})
    );
    assert_eq!(
        value["edges"][0]["target_identity"],
        serde_json::json!({"source": "acme/app", "id": "APP-001"})
    );
    assert_eq!(value["edges"][0]["provenance"]["pin"], PIN);
}

#[test]
fn packaged_schemas_declare_optional_federation_fields() {
    let viewer: Value = serde_json::from_str(export_schema("viewer").unwrap()).unwrap();
    let viewer_artifact = &viewer["properties"]["artifacts"]["items"];
    let viewer_edge = &viewer["properties"]["relationships"]["items"];
    assert!(viewer_artifact["properties"]["provenance"].is_object());
    assert!(viewer_edge["properties"]["from_identity"].is_object());
    assert!(viewer_edge["properties"]["to_identity"].is_object());
    assert!(viewer_edge["properties"]["authored_token"].is_object());
    assert!(viewer_edge["properties"]["historical_candidates"].is_object());
    assert!(viewer_edge["properties"]["effective_terminal"].is_object());
    assert!(viewer["$defs"]["override"]["properties"]["owner_source"].is_object());
    assert!(viewer["$defs"]["override"]["properties"]["state"]["enum"]
        .as_array()
        .unwrap()
        .iter()
        .any(|state| state == "lineage"));
    assert!(!viewer_artifact["required"]
        .as_array()
        .unwrap()
        .iter()
        .any(|field| field == "provenance"));

    let documents: Value = serde_json::from_str(export_schema("documents").unwrap()).unwrap();
    assert!(documents["properties"]["metadata"]["properties"]["provenance"].is_object());
    assert!(documents["$defs"]["override"]["properties"]["owner_source"].is_object());

    let graph: Value = serde_json::from_str(export_schema("graph").unwrap()).unwrap();
    let graph_node = &graph["properties"]["nodes"]["items"];
    let graph_edge = &graph["properties"]["edges"]["items"];
    assert!(graph_node["properties"]["provenance"].is_object());
    assert!(graph_edge["properties"]["source_identity"].is_object());
    assert!(graph_edge["properties"]["target_identity"].is_object());
    assert!(graph_edge["properties"]["authored_token"].is_object());
    assert!(graph_edge["properties"]["historical_candidates"].is_object());
    assert!(graph_edge["properties"]["effective_terminal"].is_object());
    assert_eq!(
        graph["$defs"]["provenance"]["properties"]["pin"]["pattern"],
        "^sha256(?:-v2)?:[0-9a-f]{64}$"
    );
    assert_eq!(
        graph["$defs"]["provenance"]["allOf"][0]["then"]["required"][0],
        "pin"
    );
}

#[test]
fn graph_exports_retain_reconciled_diamond_chain_and_endpoint_history() {
    let root = scratch("graph-diamond-export");
    let directory = corpus_path(&root, "child");
    let corpus = diamond_chain_corpus();

    let viewer =
        build_corpus_export_from_composed(&directory, "test".to_string(), &corpus, false).unwrap();
    assert_eq!(viewer.artifacts.len(), 6);
    let terminal = viewer
        .artifacts
        .iter()
        .find(|artifact| artifact.id == "ROOT-POLICY")
        .unwrap();
    assert!(terminal.provenance.is_none());
    let terminal_provenance = terminal.graph_provenance.as_ref().unwrap();
    assert_eq!(terminal_provenance.origin.source, "acme/app");
    assert_eq!(terminal_provenance.origin.layer, Layer::Local);
    assert_eq!(terminal_provenance.origin.pin, None);
    assert_eq!(terminal_provenance.overrides.len(), 3);
    assert_eq!(
        terminal_provenance
            .overrides
            .iter()
            .map(|mapping| (mapping.owner_source.as_str(), mapping.state.as_str()))
            .collect::<Vec<_>>(),
        vec![
            ("acme/left", "lineage"),
            ("acme/app", "replacement"),
            ("acme/app", "replacement"),
        ]
    );
    let history = viewer
        .artifacts
        .iter()
        .find(|artifact| artifact.id == "POLICY")
        .unwrap();
    assert_eq!(
        history.graph_provenance.as_ref().unwrap().origin.pin.as_deref(),
        Some(GRAPH_PIN)
    );

    let relationship = viewer
        .relationships
        .iter()
        .find(|relationship| relationship.from == "CONSUMER")
        .unwrap();
    assert_eq!(relationship.authored_token.as_deref(), Some("POLICY"));
    assert_eq!(
        relationship.historical_candidates,
        vec![identity("acme/shared", "POLICY")]
    );
    assert_eq!(
        relationship.effective_terminal,
        Some(identity("acme/app", "ROOT-POLICY"))
    );
    assert_eq!(
        relationship.to_identity,
        Some(identity("acme/app", "ROOT-POLICY"))
    );

    let viewer_json: Value = serde_json::from_str(&render_export_json(&viewer)).unwrap();
    let terminal_json = viewer_json["artifacts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|artifact| artifact["id"] == "ROOT-POLICY")
        .unwrap();
    assert_eq!(
        terminal_json["provenance"]["overrides"][0]["owner_source"],
        "acme/left"
    );
    assert_eq!(
        terminal_json["provenance"]["overrides"][0]["state"],
        "lineage"
    );
    assert_eq!(
        terminal_json["provenance"]["overrides"][0]["target"],
        serde_json::json!({"source": "acme/shared", "id": "POLICY"})
    );
    assert!(terminal_json["provenance"]["overrides"][0]
        .get("parent")
        .is_none());
    assert_eq!(
        viewer_json["relationships"][0]["historical_candidates"],
        serde_json::json!([{"source": "acme/shared", "id": "POLICY"}])
    );
    assert_eq!(
        viewer_json["relationships"][0]["effective_terminal"],
        serde_json::json!({"source": "acme/app", "id": "ROOT-POLICY"})
    );

    let documents = build_documents_export_from_composed(&directory, &corpus, false).unwrap();
    assert_eq!(documents.documents.len(), 6);
    let terminal_document = documents
        .documents
        .iter()
        .find(|document| document.id == "ROOT-POLICY")
        .unwrap();
    assert_eq!(
        terminal_document
            .graph_provenance
            .as_ref()
            .unwrap()
            .overrides,
        terminal_provenance.overrides
    );
    let rendered_document: Value = render_documents_jsonl(&documents)
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .find(|document| document["id"] == "ROOT-POLICY")
        .unwrap();
    assert_eq!(
        rendered_document["metadata"]["provenance"]["overrides"].as_array().unwrap().len(),
        3
    );

    let graph = build_graph_export_from_composed(&directory, &corpus, false).unwrap();
    assert_eq!(graph.nodes.len(), 6);
    let edge = graph
        .edges
        .iter()
        .find(|edge| edge.source == "CONSUMER")
        .unwrap();
    assert_eq!(edge.authored_token.as_deref(), Some("POLICY"));
    assert_eq!(
        edge.graph_provenance.as_ref().unwrap().origin.pin.as_deref(),
        Some(GRAPH_PIN)
    );
    assert_eq!(
        edge.historical_candidates,
        vec![identity("acme/shared", "POLICY")]
    );
    assert_eq!(
        edge.effective_terminal,
        Some(identity("acme/app", "ROOT-POLICY"))
    );
    let graph_json: Value = serde_json::from_str(&render_graph_json(&graph)).unwrap();
    let edge_json = graph_json["edges"]
        .as_array()
        .unwrap()
        .iter()
        .find(|edge| edge["source"] == "CONSUMER")
        .unwrap();
    assert_eq!(edge_json["authored_token"], "POLICY");
    assert_eq!(
        edge_json["effective_terminal"],
        serde_json::json!({"source": "acme/app", "id": "ROOT-POLICY"})
    );

    let local_viewer =
        build_corpus_export_from_composed(&directory, "test".to_string(), &corpus, true).unwrap();
    assert_eq!(
        local_viewer
            .artifacts
            .iter()
            .map(|artifact| artifact.id.as_str())
            .collect::<std::collections::BTreeSet<_>>(),
        std::collections::BTreeSet::from(["ROOT-ADR", "ROOT-POLICY"])
    );
    assert!(local_viewer.artifacts.iter().all(|artifact| {
        artifact
            .graph_provenance
            .as_ref()
            .is_some_and(|provenance| provenance.origin.layer == Layer::Local)
    }));
    assert_eq!(
        build_documents_export_from_composed(&directory, &corpus, true)
            .unwrap()
            .documents
            .len(),
        2
    );
    assert_eq!(
        build_graph_export_from_composed(&directory, &corpus, true)
            .unwrap()
            .nodes
            .len(),
        2
    );

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn graph_portal_keeps_three_equal_ids_independently_addressable() {
    let root = scratch("graph-equal-id-portal");
    let directory = corpus_path(&root, "child");
    let shared = "acme/shared";
    let left = "acme/left";
    let right = "acme/right";
    let corpus = graph_corpus(
        vec![
            graph_node(
                "acme/app",
                vec![graph_parent(left, "left"), graph_parent(right, "right")],
            ),
            graph_node(left, vec![graph_parent(shared, "base")]),
            graph_node(right, vec![graph_parent(shared, "base")]),
            graph_node(shared, Vec::new()),
        ],
        vec![
            graph_item(shared, "shared.md", "SAME", "requirement", ""),
            graph_item(left, "left.md", "SAME", "requirement", ""),
            graph_item(right, "right.md", "SAME", "requirement", ""),
            graph_item(
                "acme/app",
                "replacement.md",
                "ROOT-SAME",
                "requirement",
                "",
            ),
            graph_item("acme/app", "rationale.md", "ROOT-ADR", "decision", ""),
            graph_item(
                "acme/app",
                "consumer.md",
                "ROOT-CONSUMER",
                "requirement",
                "## Related Requirements\n\n- SAME\n",
            ),
        ],
        vec![
            graph_mapping("acme/app", left, "SAME", "ROOT-SAME", "ROOT-ADR"),
            graph_mapping("acme/app", right, "SAME", "ROOT-SAME", "ROOT-ADR"),
            graph_mapping("acme/app", shared, "SAME", "ROOT-SAME", "ROOT-ADR"),
        ],
    );
    let viewer =
        build_corpus_export_from_composed(&directory, "test".to_string(), &corpus, false).unwrap();
    assert_eq!(viewer.artifacts.len(), 6);
    assert_eq!(
        viewer
            .artifacts
            .iter()
            .filter(|artifact| artifact.id == "SAME")
            .map(|artifact| {
                let provenance = artifact.graph_provenance.as_ref().unwrap();
                format!("{}::{}", provenance.origin.source, artifact.id)
            })
            .collect::<std::collections::BTreeSet<_>>(),
        std::collections::BTreeSet::from([
            "acme/left::SAME".to_string(),
            "acme/right::SAME".to_string(),
            "acme/shared::SAME".to_string(),
        ])
    );
    let relationship = viewer
        .relationships
        .iter()
        .find(|relationship| relationship.from == "ROOT-CONSUMER")
        .unwrap();
    assert_eq!(relationship.authored_token.as_deref(), Some("SAME"));
    assert_eq!(
        relationship.historical_candidates,
        vec![
            identity(left, "SAME"),
            identity(right, "SAME"),
            identity(shared, "SAME"),
        ]
    );
    assert_eq!(
        relationship.effective_terminal,
        Some(identity("acme/app", "ROOT-SAME"))
    );

    let html = render_federated_export_html(&viewer).unwrap();
    for source in [left, right, shared] {
        assert!(html.contains(&format!("\"source\": \"{source}\"")));
    }
    assert!(FEDERATED_SHELL.contains("function Ki(c){return`${c.source}::${c.id}`}"));
    assert!(FEDERATED_SHELL.contains("#/artifact/${encodeURIComponent(J)}"));
    assert!(FEDERATED_SHELL.contains("T.state===\"replacement\"&&b(T.parent.id,$)"));
    assert!(!FEDERATED_SHELL.contains("T.state!==\"overridden\""));
    assert!(html.contains(
        "T.state!==\"overridden\"&&b((T.target??T.parent).id,$)"
    ));

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn composed_exports_retain_parent_history_and_source_aware_edges() {
    let root = scratch("composed");
    let directory = corpus_path(&root, "child");
    let corpus = composed_fixture(&root);
    assert!(
        corpus.findings().is_empty(),
        "unexpected findings: {:?}",
        corpus.findings()
    );

    let viewer =
        build_corpus_export_from_composed(&directory, "test".to_string(), &corpus, false).unwrap();
    let v1_html = render_federated_export_html(&viewer).unwrap();
    assert!(v1_html.contains("T.state===\"replacement\"&&b(T.parent.id,$)"));
    assert!(!v1_html.contains("T.state!==\"overridden\""));
    assert_eq!(viewer.artifacts.len(), 4);
    let parent = viewer
        .artifacts
        .iter()
        .find(|artifact| artifact.id == "STD-001")
        .unwrap();
    assert_eq!(parent.path, "policy.md");
    let parent_provenance = parent.provenance.as_ref().unwrap();
    assert_eq!(parent_provenance.origin.source, "acme/standards");
    assert_eq!(parent_provenance.origin.layer, Layer::Inherited);
    assert_eq!(parent_provenance.origin.pin.as_deref(), Some(PIN));
    assert_eq!(parent_provenance.overrides[0].state.as_str(), "overridden");

    let replacement = viewer
        .artifacts
        .iter()
        .find(|artifact| artifact.id == "APP-001")
        .unwrap();
    let replacement_provenance = replacement.provenance.as_ref().unwrap();
    assert_eq!(replacement_provenance.origin.source, "acme/app");
    assert_eq!(replacement_provenance.origin.layer, Layer::Local);
    assert_eq!(replacement_provenance.origin.pin, None);
    assert_eq!(
        replacement_provenance.overrides[0].state.as_str(),
        "replacement"
    );
    let parent_edge = viewer
        .relationships
        .iter()
        .find(|edge| edge.from == "STD-001")
        .unwrap();
    assert_eq!(
        parent_edge.from_identity,
        Some(identity("acme/standards", "STD-001"))
    );
    assert_eq!(
        parent_edge.to_identity,
        Some(identity("acme/standards", "STD-002"))
    );

    let documents = build_documents_export_from_composed(&directory, &corpus, false).unwrap();
    assert_eq!(documents.documents.len(), 4);
    let document = documents
        .documents
        .iter()
        .find(|document| document.id == "STD-001")
        .unwrap();
    assert_eq!(
        document.provenance.as_ref().unwrap().origin.source,
        "acme/standards"
    );
    assert!(document.text.contains("Use the shared policy."));

    let graph = build_graph_export_from_composed(&directory, &corpus, false).unwrap();
    assert_eq!(graph.nodes.len(), 4);
    assert!(graph.edges.iter().any(|edge| {
        edge.source_identity == Some(identity("acme/standards", "STD-001"))
            && edge.target_identity == Some(identity("acme/standards", "STD-002"))
    }));

    let local =
        build_corpus_export_from_composed(&directory, "test".to_string(), &corpus, true).unwrap();
    assert_eq!(local.artifacts.len(), 2);
    assert!(local
        .artifacts
        .iter()
        .all(|artifact| { artifact.provenance.as_ref().unwrap().origin.layer == Layer::Local }));

    let okf = build_okf_export_from_composed(&directory, "test".to_string(), &corpus);
    assert_eq!(
        okf.artifacts
            .iter()
            .map(|artifact| artifact.id.as_str())
            .collect::<std::collections::BTreeSet<_>>(),
        std::collections::BTreeSet::from(["APP-001", "APP-ADR-001"])
    );

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn same_id_override_retains_both_source_qualified_viewer_records() {
    let root = scratch("same-id");
    let child_directory = corpus_path(&root, "child");
    let parent_directory = corpus_path(&root, "parent");
    fs::write(
        root.join("child/decisions/replacement.md"),
        LOCAL_REPLACEMENT.replace("APP-001", "STD-001"),
    )
    .unwrap();

    let local = corpus_items(&child_directory, true);
    let inherited = inherited_items(&parent_directory);
    let content: Vec<_> = local
        .iter()
        .chain(&inherited)
        .map(|item| (item.key.clone(), fs::read(&item.locator.path).unwrap()))
        .collect();
    let corpus = ComposedCorpus::compose_with_content(
        local,
        inherited,
        ParentIdentity::new("acme/standards", "standards").unwrap(),
        vec![OverrideDeclaration::parse("standards::STD-001", "STD-001", "APP-ADR-001").unwrap()],
        content,
    );
    assert!(
        corpus.findings().is_empty(),
        "unexpected findings: {:?}",
        corpus.findings()
    );

    let viewer =
        build_corpus_export_from_composed(&child_directory, "test".to_string(), &corpus, false)
            .unwrap();
    let shared: std::collections::BTreeSet<_> = viewer
        .artifacts
        .iter()
        .filter(|artifact| artifact.id == "STD-001")
        .map(|artifact| artifact.provenance.as_ref().unwrap().origin.source.as_str())
        .collect();
    assert_eq!(
        shared,
        std::collections::BTreeSet::from(["acme/app", "acme/standards"])
    );

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn local_composed_model_delegates_to_byte_identical_legacy_exports() {
    let root = scratch("no-manifest");
    let directory = corpus_path(&root, "child");
    let legacy_viewer = build_corpus_export(&directory, "test".to_string()).unwrap();
    let legacy_documents = build_documents_export(&directory).unwrap();
    let legacy_graph = build_graph_export(&directory).unwrap();
    let local = ComposedCorpus::local(corpus_items(&directory, true));
    let composed_viewer =
        build_corpus_export_from_composed(&directory, "test".to_string(), &local, false).unwrap();
    let composed_documents =
        build_documents_export_from_composed(&directory, &local, false).unwrap();
    let composed_graph = build_graph_export_from_composed(&directory, &local, false).unwrap();

    assert_eq!(
        render_export_json(&composed_viewer),
        render_export_json(&legacy_viewer)
    );
    assert_eq!(
        render_documents_jsonl(&composed_documents),
        render_documents_jsonl(&legacy_documents)
    );
    assert_eq!(
        render_graph_json(&composed_graph),
        render_graph_json(&legacy_graph)
    );
    assert_eq!(
        render_export_html(&composed_viewer).unwrap(),
        render_export_html(&legacy_viewer).unwrap()
    );
    assert_eq!(
        rac_engine::sha256::hexdigest(SHELL.as_bytes()),
        "91e5268e47a8b094cf3f393cdfddf9204ae6b6b9059652780931fff6d3e4abb3"
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn repeated_source_identity_is_an_explicit_export_conflict() {
    let root = scratch("conflict");
    let child_directory = corpus_path(&root, "child");
    let parent_directory = corpus_path(&root, "parent");
    let conflicting = LOCAL_REPLACEMENT.replace(
        "Replace the parent policy locally.",
        "Disagree with the other record body.",
    );
    fs::write(
        root.join("child/decisions/replacement-copy.md"),
        conflicting,
    )
    .unwrap();
    let local = corpus_items(&child_directory, true);
    let inherited = inherited_items(&parent_directory);
    let content: Vec<_> = local
        .iter()
        .chain(&inherited)
        .map(|item| (item.key.clone(), fs::read(&item.locator.path).unwrap()))
        .collect();
    let corpus = ComposedCorpus::compose_with_content(
        local,
        inherited,
        ParentIdentity::new("acme/standards", "standards").unwrap(),
        Vec::new(),
        content,
    );

    let error = build_documents_export_from_composed(&child_directory, &corpus, true)
        .err()
        .expect("duplicate source identity must fail");
    assert_eq!(
        error.message(),
        "federated-export-record-conflict: acme/app::APP-001 has disagreeing path, body, or pin"
    );
    fs::remove_dir_all(root).unwrap();
}
