use std::path::{Path, PathBuf};

use rac_engine::composition::{ComposedCorpus, ParentIdentity};
use rac_engine::corpus::{
    ArtifactKey, CorpusLayer, PhysicalArtifactLocator, PhysicalCorpusLocator,
};
use rac_engine::graph_composition::{
    GraphComposition, GraphCompositionInput, GraphOverrideDeclaration, GraphOverrideState,
    SourceNodeInput, SourceParentInput,
};
use rac_engine::parse::parse_text;
use rac_engine::relationships::CorpusItem;
use rac_engine::resolve::OUTCOME_RESOLVED;
use rac_engine::spec::spec_for;

const ROOT: &str = "acme/app";
const PARENT: &str = "acme/standards";

fn key(source: &str, id: &str) -> ArtifactKey {
    ArtifactKey::new(source, id)
}

fn item(
    source: &str,
    relative_path: &str,
    id: &str,
    artifact_type: &str,
    relationships: &str,
) -> CorpusItem {
    let body = match artifact_type {
        "decision" => {
            "## Context\n\nFacade fixture.\n\n## Decision\n\nKeep graph reads central.\n\n## Consequences\n\nConsumers share one projection.\n"
        }
        "requirement" => {
            "## Problem\n\nGraph activation needs a compatibility facade.\n\n## Requirements\n\n- [REQ-001] Reads MUST share one graph.\n"
        }
        other => panic!("unsupported fixture type {other}"),
    };
    let text = format!(
        "---\nschema_version: 1\ntype: {artifact_type}\n---\n# {id}\n\n## ID\n\n{id}\n\n## Status\n\nAccepted\n\n{body}\n{relationships}"
    );
    let origin = if source == ROOT {
        CorpusLayer::local(source).origin()
    } else {
        CorpusLayer::inherited(source, "standards", "sha256-v2:fixture").origin()
    };
    let display = format!("/runtime/{source}/{relative_path}");
    CorpusItem::new(
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
    )
}

fn graph() -> GraphComposition {
    GraphComposition::compose(GraphCompositionInput::new(
        ROOT,
        vec![
            SourceNodeInput::new(ROOT, vec![SourceParentInput::new(PARENT, "standards")]),
            SourceNodeInput::new(PARENT, Vec::new()),
        ],
        vec![
            item(PARENT, "policy.md", "STD-POLICY", "requirement", ""),
            item(
                PARENT,
                "consumer.md",
                "STD-CONSUMER",
                "requirement",
                "## Related Requirements\n\n- STD-POLICY\n",
            ),
            item(ROOT, "replacement.md", "APP-POLICY", "requirement", ""),
            item(ROOT, "rationale.md", "APP-ADR", "decision", ""),
        ],
        vec![GraphOverrideDeclaration::new(
            ROOT,
            key(PARENT, "STD-POLICY"),
            key(ROOT, "APP-POLICY"),
            key(ROOT, "APP-ADR"),
        )],
    ))
    .unwrap()
}

#[test]
fn v1_and_local_facades_keep_their_original_projection_contract() {
    let local_item = item(ROOT, "local.md", "APP-LOCAL", "requirement", "");
    let local = ComposedCorpus::local(vec![local_item]);
    assert!(!local.is_graph());
    assert!(!local.is_federated());
    assert!(local.graph().is_none());
    assert!(local.read_only_roots().is_empty());
    assert!(local.read_only_root().is_none());
    assert_eq!(local.catalog().len(), 1);
    assert_eq!(local.effective().len(), 1);
    assert_eq!(local.local_items().len(), 1);
    assert_eq!(
        local.resolve("APP-LOCAL").unwrap().key,
        key(ROOT, "APP-LOCAL")
    );
    assert_eq!(
        local.resolve_identity("APP-LOCAL").outcome,
        OUTCOME_RESOLVED
    );
    assert_eq!(local.effective_index().len(), 1);
    assert_eq!(local.identity_index().len(), 1);
    assert!(local
        .graph_provenance_for(&key(ROOT, "APP-LOCAL"))
        .is_none());

    let inherited = item(PARENT, "parent.md", "STD-PARENT", "requirement", "");
    let expected_root = inherited.locator.corpus.repository_root.clone();
    let v1 = ComposedCorpus::compose(
        Vec::new(),
        vec![inherited],
        ParentIdentity::new(PARENT, "standards").unwrap(),
        Vec::new(),
    );
    assert!(!v1.is_graph());
    assert!(v1.is_federated());
    assert_eq!(v1.read_only_root(), Some(expected_root.as_path()));
    assert_eq!(v1.read_only_roots(), &[expected_root]);
    assert_eq!(v1.catalog().len(), 1);
    assert_eq!(v1.effective().len(), 1);
}

#[test]
fn graph_facade_delegates_every_compatible_read_to_one_graph() {
    let roots = vec![
        PathBuf::from("/runtime/z-copy"),
        PathBuf::from("/runtime/a-copy"),
        PathBuf::from("/runtime/z-copy"),
    ];
    let corpus = ComposedCorpus::from_graph(
        graph(),
        vec![
            (key(PARENT, "STD-POLICY"), b"verified parent".to_vec()),
            (key(ROOT, "APP-POLICY"), b"captured root".to_vec()),
            (key("unknown/source", "MISSING"), b"discard me".to_vec()),
        ],
        roots,
    );

    assert!(corpus.is_graph());
    assert!(corpus.is_federated());
    assert!(corpus.graph().is_some());
    assert_eq!(corpus.child_source(), Some(ROOT));
    assert_eq!(corpus.read_only_root(), Some(Path::new("/runtime/a-copy")));
    assert_eq!(
        corpus.read_only_roots(),
        &[
            PathBuf::from("/runtime/a-copy"),
            PathBuf::from("/runtime/z-copy")
        ]
    );
    assert_eq!(corpus.catalog().len(), 4);
    assert_eq!(corpus.effective().len(), 3);
    assert_eq!(corpus.local_items().len(), 2);
    assert_eq!(
        corpus
            .effective()
            .map(|item| item.key.clone())
            .collect::<Vec<_>>(),
        vec![
            key(ROOT, "APP-ADR"),
            key(ROOT, "APP-POLICY"),
            key(PARENT, "STD-CONSUMER"),
        ]
    );

    assert_eq!(
        corpus.resolve("STD-POLICY").unwrap().key,
        key(ROOT, "APP-POLICY")
    );
    assert_eq!(
        corpus.resolve("acme/standards::STD-POLICY").unwrap().key,
        key(PARENT, "STD-POLICY")
    );
    assert_eq!(
        corpus.resolve_identity("STD-POLICY").outcome,
        OUTCOME_RESOLVED
    );
    assert_eq!(corpus.effective_index().len(), 3);
    assert_eq!(corpus.identity_index().len(), 4);
    assert!(corpus.is_overridden(&key(PARENT, "STD-POLICY")));
    assert_eq!(
        corpus.content(&key(PARENT, "STD-POLICY")),
        Some(b"verified parent".as_slice())
    );
    assert_eq!(corpus.captured_contents().count(), 2);

    let live = corpus
        .relationships()
        .into_iter()
        .find(|relationship| {
            relationship
                .source_artifact
                .as_ref()
                .is_some_and(|path| path.source == PARENT && path.relative_path == "consumer.md")
        })
        .unwrap();
    assert_eq!(live.resolved_artifact.unwrap().source, ROOT);
    let catalog = corpus
        .catalog_relationships()
        .into_iter()
        .find(|relationship| {
            relationship
                .source_artifact
                .as_ref()
                .is_some_and(|path| path.source == PARENT && path.relative_path == "consumer.md")
        })
        .unwrap();
    assert_eq!(catalog.resolved_artifact.unwrap().source, PARENT);
    assert!(corpus.local_relationships().is_empty());
}

#[test]
fn graph_chain_provenance_is_never_flattened_into_the_v1_carrier() {
    let corpus = ComposedCorpus::from_graph(graph(), Vec::new(), Vec::new());
    let historical = key(PARENT, "STD-POLICY");
    let replacement = key(ROOT, "APP-POLICY");

    let historical_chain = corpus.graph_provenance_for(&historical).unwrap();
    assert_eq!(historical_chain.len(), 1);
    assert_eq!(historical_chain[0].state, GraphOverrideState::Overridden);
    let replacement_chain = corpus.graph_provenance_for(&replacement).unwrap();
    assert_eq!(replacement_chain.len(), 1);
    assert_eq!(replacement_chain[0].state, GraphOverrideState::Replacement);
    let rendered = rac_engine::output::composed_artifact_provenance_value(&corpus, &replacement)
        .expect("graph provenance serializes without a v1 projection");
    assert_eq!(rendered["source"], ROOT);
    assert_eq!(rendered["overrides"][0]["state"], "replacement");
    assert_eq!(rendered["overrides"][0]["owner_source"], ROOT);
    assert_eq!(rendered["overrides"][0]["target"]["source"], PARENT);
    assert!(corpus.provenance_for(&historical).is_none());
    assert!(corpus.provenance_for(&replacement).is_none());

    let rationale = key(ROOT, "APP-ADR");
    let fixed = corpus.provenance_for(&rationale).unwrap();
    assert_eq!(fixed.origin.source, ROOT);
    assert!(fixed.overrides.is_empty());
    assert!(corpus.graph_provenance_for(&rationale).unwrap().is_empty());
}
