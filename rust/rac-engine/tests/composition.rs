use rac_engine::composition::{
    CanonicalId, ComposedCorpus, InvalidOverrideReason, LookupError, OverrideDeclaration,
    OverrideSyntaxError, ParentIdentity, FINDING_CANONICAL_COLLISION, FINDING_INVALID_OVERRIDE,
};
use rac_engine::corpus::{
    ArtifactKey, ArtifactPath, CorpusLayer, Layer, PhysicalArtifactLocator, PhysicalCorpusLocator,
};
use rac_engine::parse::parse_text;
use rac_engine::relationships::{
    CorpusItem, ISSUE_RELATIONSHIP_CYCLE, ISSUE_TARGET_NOT_FOUND, ISSUE_TARGET_TYPE_MISMATCH,
};
use rac_engine::spec::spec_for;

const LOCAL_SOURCE: &str = "acme/app";
const PARENT_SOURCE: &str = "acme/standards";
const PARENT_ALIAS: &str = "standards";

fn parent() -> ParentIdentity {
    ParentIdentity::new(PARENT_SOURCE, PARENT_ALIAS).unwrap()
}

fn item(
    layer: Layer,
    relative_path: &str,
    id: &str,
    artifact_type: &str,
    status: &str,
    relationships: &str,
) -> CorpusItem {
    let required = match artifact_type {
        "decision" => {
            r#"
## Context

Composition fixture.

## Decision

Keep resolution deterministic.

## Consequences

Every endpoint remains source-aware.
"#
        }
        "requirement" => {
            r#"
## Problem

Composition needs one resolver.

## Requirements

- [REQ-001] Resolution MUST remain deterministic.
"#
        }
        other => panic!("unsupported fixture type {other}"),
    };
    let text = format!(
        "---\nschema_version: 1\ntype: {artifact_type}\n---\n# {id}\n\n## ID\n\n{id}\n\n## Status\n\n{status}\n{required}\n{relationships}\n"
    );
    let source = match layer {
        Layer::Local => LOCAL_SOURCE,
        Layer::Inherited => PARENT_SOURCE,
    };
    let origin = match layer {
        Layer::Local => CorpusLayer::local(source).origin(),
        Layer::Inherited => {
            CorpusLayer::inherited(source, PARENT_ALIAS, "sha256:0123456789abcdef").origin()
        }
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

fn declaration(parent_id: &str, replacement: &str, rationale: &str) -> OverrideDeclaration {
    OverrideDeclaration::parse(
        &format!("{PARENT_ALIAS}::{parent_id}"),
        replacement,
        rationale,
    )
    .unwrap()
}

fn lookup_error(corpus: &ComposedCorpus, reference: &str) -> LookupError {
    match corpus.resolve(reference) {
        Ok(item) => panic!(
            "{reference} unexpectedly resolved to {}",
            item.key.canonical_id
        ),
        Err(error) => error,
    }
}

#[test]
fn collisions_are_sourced_and_never_pick_a_layer() {
    let local = item(
        Layer::Local,
        "z-local.md",
        "SHARED-001",
        "requirement",
        "Accepted",
        "",
    );
    let inherited = item(
        Layer::Inherited,
        "a-parent.md",
        "SHARED-001",
        "requirement",
        "Accepted",
        "",
    );
    let corpus = ComposedCorpus::compose(vec![local], vec![inherited], parent(), Vec::new());

    assert_eq!(corpus.findings().len(), 1);
    assert_eq!(corpus.findings()[0].code, FINDING_CANONICAL_COLLISION);
    assert_eq!(
        corpus.findings()[0].artifacts,
        vec![
            ArtifactKey::new(LOCAL_SOURCE, "SHARED-001"),
            ArtifactKey::new(PARENT_SOURCE, "SHARED-001"),
        ]
    );
    assert_eq!(
        lookup_error(&corpus, "SHARED-001"),
        LookupError::Ambiguous(vec![
            ArtifactKey::new(LOCAL_SOURCE, "SHARED-001"),
            ArtifactKey::new(PARENT_SOURCE, "SHARED-001"),
        ])
    );
    assert_eq!(
        corpus
            .catalog()
            .map(|entry| entry.artifact_path.clone())
            .collect::<Vec<_>>(),
        vec![
            ArtifactPath::new(LOCAL_SOURCE, "z-local.md"),
            ArtifactPath::new(PARENT_SOURCE, "a-parent.md"),
        ]
    );
}

#[test]
fn aliases_are_unique_only_and_qualification_requires_a_canonical_id() {
    let local = item(
        Layer::Local,
        "shared.md",
        "APP-001",
        "requirement",
        "Accepted",
        "",
    );
    let inherited = item(
        Layer::Inherited,
        "shared.md",
        "STD-001",
        "requirement",
        "Accepted",
        "",
    );
    let corpus = ComposedCorpus::compose(vec![local], vec![inherited], parent(), Vec::new());

    assert!(matches!(
        corpus.resolve("shared"),
        Err(LookupError::Ambiguous(keys)) if keys.len() == 2
    ));
    assert_eq!(
        lookup_error(&corpus, "standards::shared"),
        LookupError::QualifiedCanonicalRequired
    );
    assert_eq!(
        lookup_error(&corpus, "Standards::STD-001"),
        LookupError::NotFound
    );
    let resolved = corpus.resolve("standards::std-001").unwrap();
    assert_eq!(resolved.key, ArtifactKey::new(PARENT_SOURCE, "STD-001"));
}

#[test]
fn a_valid_override_redirects_only_the_parent_canonical_id_and_retains_history() {
    let replacement = item(
        Layer::Local,
        "replacement.md",
        "APP-REQ",
        "requirement",
        "Accepted",
        "",
    );
    let rationale = item(
        Layer::Local,
        "rationale.md",
        "APP-ADR",
        "decision",
        "Accepted",
        "",
    );
    let inherited = item(
        Layer::Inherited,
        "parent-policy.md",
        "STD-REQ",
        "requirement",
        "Accepted",
        "## Related Decisions\n\n- APP-ADR\n",
    );
    let replacement_key = replacement.key.clone();
    let inherited_key = inherited.key.clone();
    let corpus = ComposedCorpus::compose_with_content(
        vec![replacement, rationale],
        vec![inherited],
        parent(),
        vec![declaration("STD-REQ", "APP-REQ", "APP-ADR")],
        vec![
            (replacement_key.clone(), b"exact local bytes".to_vec()),
            (
                inherited_key.clone(),
                b"exact verified parent bytes".to_vec(),
            ),
        ],
    );

    assert!(corpus.findings().is_empty());
    assert_eq!(corpus.local_items().len(), 2);
    assert_eq!(corpus.catalog().len(), 3);
    assert_eq!(corpus.effective().len(), 2);
    assert_eq!(
        corpus.resolve("STD-REQ").unwrap().key,
        ArtifactKey::new(LOCAL_SOURCE, "APP-REQ")
    );
    assert_eq!(
        corpus.resolve("standards::STD-REQ").unwrap().key,
        ArtifactKey::new(PARENT_SOURCE, "STD-REQ")
    );
    assert_eq!(
        lookup_error(&corpus, "parent-policy"),
        LookupError::NotFound
    );
    assert_eq!(
        lookup_error(&corpus, "standards::parent-policy"),
        LookupError::QualifiedCanonicalRequired
    );
    assert!(corpus.is_overridden(&ArtifactKey::new(PARENT_SOURCE, "STD-REQ")));
    assert_eq!(
        corpus.overrides()[0].rationale,
        ArtifactKey::new(LOCAL_SOURCE, "APP-ADR")
    );
    assert_eq!(
        corpus.content(&replacement_key),
        Some(&b"exact local bytes"[..])
    );
    assert_eq!(
        corpus.content(&inherited_key),
        Some(&b"exact verified parent bytes"[..])
    );
    assert!(!corpus
        .relationships()
        .iter()
        .any(|edge| edge.source_artifact
            == Some(ArtifactPath::new(PARENT_SOURCE, "parent-policy.md"))));
    assert!(corpus
        .catalog_relationships()
        .iter()
        .any(|edge| edge.source_artifact
            == Some(ArtifactPath::new(PARENT_SOURCE, "parent-policy.md"))));
}

#[test]
fn a_same_id_override_is_the_only_way_to_clear_its_collision() {
    let replacement = item(
        Layer::Local,
        "replacement.md",
        "POLICY-001",
        "requirement",
        "Accepted",
        "",
    );
    let rationale = item(
        Layer::Local,
        "rationale.md",
        "APP-ADR",
        "decision",
        "Accepted",
        "",
    );
    let inherited = item(
        Layer::Inherited,
        "policy.md",
        "POLICY-001",
        "requirement",
        "Accepted",
        "",
    );
    let corpus = ComposedCorpus::compose(
        vec![replacement, rationale],
        vec![inherited],
        parent(),
        vec![declaration("POLICY-001", "POLICY-001", "APP-ADR")],
    );

    assert!(corpus.findings().is_empty());
    assert_eq!(
        corpus.resolve("POLICY-001").unwrap().key,
        ArtifactKey::new(LOCAL_SOURCE, "POLICY-001")
    );
    assert_eq!(
        corpus.resolve("standards::POLICY-001").unwrap().key,
        ArtifactKey::new(PARENT_SOURCE, "POLICY-001")
    );
}

#[test]
fn override_operands_are_canonical_local_and_decision_backed() {
    assert_eq!(
        CanonicalId::new("standards::STD-REQ"),
        Err(OverrideSyntaxError::QualifiedLocalId)
    );

    let replacement = item(
        Layer::Local,
        "replacement-alias.md",
        "APP-REQ",
        "requirement",
        "Accepted",
        "",
    );
    let draft_rationale = item(
        Layer::Local,
        "rationale.md",
        "APP-ADR",
        "decision",
        "Proposed",
        "",
    );
    let inherited = item(
        Layer::Inherited,
        "policy.md",
        "STD-REQ",
        "requirement",
        "Accepted",
        "",
    );
    let alias_operand = ComposedCorpus::compose(
        vec![replacement.clone(), draft_rationale.clone()],
        vec![inherited.clone()],
        parent(),
        vec![declaration("STD-REQ", "replacement-alias", "APP-ADR")],
    );
    assert_eq!(alias_operand.findings()[0].code, FINDING_INVALID_OVERRIDE);
    assert_eq!(
        alias_operand.findings()[0].reason,
        Some(InvalidOverrideReason::ReplacementNotFound)
    );

    let dead_rationale = ComposedCorpus::compose(
        vec![replacement, draft_rationale],
        vec![inherited],
        parent(),
        vec![declaration("STD-REQ", "APP-REQ", "APP-ADR")],
    );
    assert_eq!(dead_rationale.findings()[0].code, FINDING_INVALID_OVERRIDE);
    assert_eq!(
        dead_rationale.findings()[0].reason,
        Some(InvalidOverrideReason::RationaleNotLive)
    );
}

#[test]
fn cross_source_relationships_resolve_to_typed_endpoints_and_check_types() {
    let local = item(
        Layer::Local,
        "child-requirement.md",
        "APP-REQ",
        "requirement",
        "Accepted",
        "## Related Decisions\n\n- standards::STD-ADR\n",
    );
    let parent_decision = item(
        Layer::Inherited,
        "parent-decision.md",
        "STD-ADR",
        "decision",
        "Accepted",
        "",
    );
    let corpus = ComposedCorpus::compose(vec![local], vec![parent_decision], parent(), Vec::new());
    let edge = corpus
        .relationships()
        .into_iter()
        .find(|edge| edge.relationship == "related_decisions")
        .unwrap();
    assert_eq!(
        edge.source_artifact,
        Some(ArtifactPath::new(LOCAL_SOURCE, "child-requirement.md"))
    );
    assert_eq!(
        edge.resolved_artifact,
        Some(ArtifactPath::new(PARENT_SOURCE, "parent-decision.md"))
    );
    assert!(edge.issue.is_none());

    let uppercase_alias = ComposedCorpus::compose(
        vec![item(
            Layer::Local,
            "uppercase.md",
            "APP-UPPER",
            "requirement",
            "Accepted",
            "## Related Decisions\n\n- Standards::STD-ADR\n",
        )],
        vec![item(
            Layer::Inherited,
            "parent-decision.md",
            "STD-ADR",
            "decision",
            "Accepted",
            "",
        )],
        parent(),
        Vec::new(),
    );
    assert_eq!(
        uppercase_alias.relationships()[0].issue.as_deref(),
        Some(ISSUE_TARGET_NOT_FOUND)
    );

    let wrong_type = item(
        Layer::Inherited,
        "parent-requirement.md",
        "STD-REQ",
        "requirement",
        "Accepted",
        "",
    );
    let local = item(
        Layer::Local,
        "child-requirement.md",
        "APP-REQ",
        "requirement",
        "Accepted",
        "## Related Decisions\n\n- STD-REQ\n",
    );
    let corpus = ComposedCorpus::compose(vec![local], vec![wrong_type], parent(), Vec::new());
    assert!(corpus
        .validate_relationships(".", true)
        .issues
        .iter()
        .any(|issue| issue.code == ISSUE_TARGET_TYPE_MISMATCH));
}

#[test]
fn cross_source_cycles_are_computed_over_artifact_keys() {
    let local = item(
        Layer::Local,
        "local.md",
        "APP-ADR",
        "decision",
        "Accepted",
        "## Supersedes\n\n- standards::STD-ADR\n",
    );
    let inherited = item(
        Layer::Inherited,
        "parent.md",
        "STD-ADR",
        "decision",
        "Accepted",
        "## Supersedes\n\n- APP-ADR\n",
    );
    let corpus = ComposedCorpus::compose(vec![local], vec![inherited], parent(), Vec::new());
    let validation = corpus.validate_relationships(".", true);
    let cycle = validation
        .issues
        .iter()
        .find(|issue| issue.code == ISSUE_RELATIONSHIP_CYCLE)
        .expect("cross-source cycle");
    assert_eq!(
        cycle.paths.as_ref().unwrap(),
        &vec![
            "/runtime/acme/app/local.md".to_string(),
            "/runtime/acme/standards/parent.md".to_string(),
        ]
    );
}
