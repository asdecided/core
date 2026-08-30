use rac_engine::corpus::{
    ArtifactKey, CorpusLayer, Layer, PhysicalArtifactLocator, PhysicalCorpusLocator,
};
use rac_engine::graph_composition::{
    GraphComposition, GraphCompositionInput, GraphFindingReason, GraphLookupError,
    GraphOverrideDeclaration, GraphOverrideState, GraphRelationshipIssue, SourceNodeInput,
    SourceParentInput, FINDING_OVERRIDE_DIVERGENCE,
};
use rac_engine::parse::parse_text;
use rac_engine::relationships::CorpusItem;
use rac_engine::resolve::{OUTCOME_AMBIGUOUS, OUTCOME_RESOLVED};
use rac_engine::spec::spec_for;

const ROOT: &str = "acme/app";
const SHARED: &str = "acme/shared";
const LEFT: &str = "acme/left";
const RIGHT: &str = "acme/right";

fn parent(source: &str, alias: &str) -> SourceParentInput {
    SourceParentInput::new(source, alias)
}

fn node(source: &str, parents: Vec<SourceParentInput>) -> SourceNodeInput {
    SourceNodeInput::new(source, parents)
}

fn item(
    source: &str,
    relative_path: &str,
    id: &str,
    artifact_type: &str,
    status: &str,
    relationships: &str,
) -> CorpusItem {
    let body = match artifact_type {
        "decision" => {
            "## Context\n\nGraph fixture.\n\n## Decision\n\nKeep composition deterministic.\n\n## Consequences\n\nEvery exception remains attributable.\n"
        }
        "requirement" => {
            "## Problem\n\nGraph composition needs one resolver.\n\n## Requirements\n\n- [REQ-001] Resolution MUST remain deterministic.\n"
        }
        other => panic!("unsupported fixture type {other}"),
    };
    let text = format!(
        "---\nschema_version: 1\ntype: {artifact_type}\n---\n# {id}\n\n## ID\n\n{id}\n\n## Status\n\n{status}\n\n{body}\n{relationships}"
    );
    let origin = if source == ROOT {
        CorpusLayer::local(source).origin()
    } else {
        CorpusLayer::inherited(source, "runtime-only", format!("sha256-v2:{source}")).origin()
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

fn key(source: &str, id: &str) -> ArtifactKey {
    ArtifactKey::new(source, id)
}

fn mapping(
    owner: &str,
    target: (&str, &str),
    replacement: &str,
    rationale: &str,
) -> GraphOverrideDeclaration {
    GraphOverrideDeclaration::new(
        owner,
        key(target.0, target.1),
        key(owner, replacement),
        key(owner, rationale),
    )
}

fn diamond_nodes() -> Vec<SourceNodeInput> {
    vec![
        node(ROOT, vec![parent(LEFT, "left"), parent(RIGHT, "right")]),
        node(LEFT, vec![parent(SHARED, "base")]),
        node(RIGHT, vec![parent(SHARED, "base")]),
        node(SHARED, Vec::new()),
    ]
}

#[test]
fn source_context_keeps_aliases_local_and_global_qualification_stable() {
    const LEFT_BASE: &str = "acme/left-base";
    const RIGHT_BASE: &str = "acme/right-base";
    let graph = GraphComposition::compose(GraphCompositionInput::new(
        ROOT,
        vec![
            node(ROOT, vec![parent(LEFT, "left"), parent(RIGHT, "right")]),
            node(LEFT, vec![parent(LEFT_BASE, "base")]),
            node(RIGHT, vec![parent(RIGHT_BASE, "base")]),
            node(LEFT_BASE, Vec::new()),
            node(RIGHT_BASE, Vec::new()),
        ],
        vec![
            item(
                LEFT_BASE,
                "left.md",
                "LEFT-BASE",
                "requirement",
                "Accepted",
                "",
            ),
            item(
                RIGHT_BASE,
                "right.md",
                "RIGHT-BASE",
                "requirement",
                "Accepted",
                "",
            ),
        ],
        Vec::new(),
    ))
    .unwrap();

    assert_eq!(
        graph
            .resolve_from(LEFT, "base::LEFT-BASE")
            .unwrap()
            .selected,
        key(LEFT_BASE, "LEFT-BASE")
    );
    assert_eq!(
        graph
            .resolve_from(RIGHT, "base::RIGHT-BASE")
            .unwrap()
            .selected,
        key(RIGHT_BASE, "RIGHT-BASE")
    );
    assert_eq!(
        graph.resolve_from(LEFT, "base::RIGHT-BASE"),
        Err(GraphLookupError::NotFound)
    );
    assert_eq!(
        graph
            .resolve_public("acme/right-base::RIGHT-BASE")
            .unwrap()
            .selected,
        key(RIGHT_BASE, "RIGHT-BASE")
    );
    assert_eq!(
        graph.resolve_public("base::LEFT-BASE"),
        Err(GraphLookupError::NotFound),
        "an inherited alias must not leak to the root"
    );
}

#[test]
fn equal_ids_are_legal_and_bare_lookup_is_deterministically_ambiguous() {
    let graph = GraphComposition::compose(GraphCompositionInput::new(
        ROOT,
        diamond_nodes(),
        vec![
            item(SHARED, "same.md", "SAME", "requirement", "Accepted", ""),
            item(LEFT, "same.md", "SAME", "requirement", "Accepted", ""),
            item(RIGHT, "same.md", "SAME", "requirement", "Accepted", ""),
        ],
        Vec::new(),
    ))
    .unwrap();

    assert_eq!(graph.catalog().len(), 3);
    assert_eq!(
        graph.resolve_public("SAME"),
        Err(GraphLookupError::Ambiguous {
            historical_candidates: vec![key(LEFT, "SAME"), key(RIGHT, "SAME"), key(SHARED, "SAME"),],
            effective_candidates: vec![key(LEFT, "SAME"), key(RIGHT, "SAME"), key(SHARED, "SAME"),],
        })
    );
    assert_eq!(graph.resolve_identity("SAME").outcome, OUTCOME_AMBIGUOUS);
    let qualified = graph.resolve_public("acme/shared::SAME").unwrap();
    assert!(qualified.qualified);
    assert_eq!(qualified.selected, key(SHARED, "SAME"));
}

#[test]
fn explicit_multi_candidate_convergence_makes_bare_lookup_unique() {
    let mut items = vec![
        item(SHARED, "same.md", "SAME", "requirement", "Accepted", ""),
        item(LEFT, "same.md", "SAME", "requirement", "Accepted", ""),
        item(RIGHT, "same.md", "SAME", "requirement", "Accepted", ""),
        item(
            ROOT,
            "replacement.md",
            "ROOT-SAME",
            "requirement",
            "Accepted",
            "",
        ),
        item(ROOT, "rationale.md", "ROOT-ADR", "decision", "Accepted", ""),
    ];
    items.push(item(
        ROOT,
        "relationship.md",
        "ROOT-REL",
        "requirement",
        "Accepted",
        "## Related Requirements\n\n- SAME\n",
    ));
    let graph = GraphComposition::compose(GraphCompositionInput::new(
        ROOT,
        diamond_nodes(),
        items,
        vec![
            mapping(ROOT, (LEFT, "SAME"), "ROOT-SAME", "ROOT-ADR"),
            mapping(ROOT, (RIGHT, "SAME"), "ROOT-SAME", "ROOT-ADR"),
            mapping(ROOT, (SHARED, "SAME"), "ROOT-SAME", "ROOT-ADR"),
        ],
    ))
    .unwrap();

    let resolved = graph.resolve_public("SAME").unwrap();
    assert_eq!(resolved.selected, key(ROOT, "ROOT-SAME"));
    assert_eq!(resolved.historical_candidates.len(), 3);
    assert_eq!(graph.terminal_redirects().len(), 3);

    let relationship = graph
        .effective_relationships()
        .into_iter()
        .find(|relationship| relationship.source == key(ROOT, "ROOT-REL"))
        .unwrap();
    assert_eq!(relationship.historical_candidates.len(), 3);
    assert_eq!(
        relationship.effective_terminal,
        Some(key(ROOT, "ROOT-SAME"))
    );
    assert_eq!(relationship.issue, None);

    assert_eq!(graph.resolve_identity("SAME").outcome, OUTCOME_RESOLVED);
    let identity = graph.identity_index();
    let replacement = identity
        .iter()
        .find(|entry| entry.key.as_ref() == Some(&key(ROOT, "ROOT-SAME")))
        .unwrap();
    assert!(replacement.aliases.iter().any(|alias| alias == "SAME"));
}

#[test]
fn incomplete_diamond_override_is_a_blocking_divergence() {
    let result = GraphComposition::compose(GraphCompositionInput::new(
        ROOT,
        diamond_nodes(),
        vec![
            item(SHARED, "policy.md", "POLICY", "requirement", "Accepted", ""),
            item(
                LEFT,
                "replacement.md",
                "LEFT-POLICY",
                "requirement",
                "Accepted",
                "",
            ),
            item(LEFT, "rationale.md", "LEFT-ADR", "decision", "Accepted", ""),
        ],
        vec![mapping(LEFT, (SHARED, "POLICY"), "LEFT-POLICY", "LEFT-ADR")],
    ));
    let findings = match result {
        Ok(_) => panic!("an unreconciled branch fork must fail"),
        Err(findings) => findings,
    };
    assert!(findings.iter().any(|finding| {
        finding.code == FINDING_OVERRIDE_DIVERGENCE
            && finding.reason == GraphFindingReason::DivergentTerminal
            && finding.owner_source.as_deref() == Some(ROOT)
            && finding.artifacts == vec![key(LEFT, "LEFT-POLICY"), key(SHARED, "POLICY")]
    }));
}

#[test]
fn reconciled_chain_has_bottom_up_terminals_and_total_provenance() {
    let graph = GraphComposition::compose(GraphCompositionInput::new(
        ROOT,
        diamond_nodes(),
        vec![
            item(SHARED, "policy.md", "POLICY", "requirement", "Accepted", ""),
            item(
                LEFT,
                "replacement.md",
                "LEFT-POLICY",
                "requirement",
                "Accepted",
                "",
            ),
            item(LEFT, "rationale.md", "LEFT-ADR", "decision", "Accepted", ""),
            item(
                ROOT,
                "replacement.md",
                "ROOT-POLICY",
                "requirement",
                "Accepted",
                "",
            ),
            item(ROOT, "rationale.md", "ROOT-ADR", "decision", "Accepted", ""),
        ],
        vec![
            mapping(LEFT, (SHARED, "POLICY"), "LEFT-POLICY", "LEFT-ADR"),
            mapping(ROOT, (LEFT, "LEFT-POLICY"), "ROOT-POLICY", "ROOT-ADR"),
            mapping(ROOT, (SHARED, "POLICY"), "ROOT-POLICY", "ROOT-ADR"),
        ],
    ))
    .unwrap();

    assert_eq!(
        graph.resolve_public("POLICY").unwrap().selected,
        key(ROOT, "ROOT-POLICY")
    );
    let history = graph.resolve_public("acme/shared::POLICY").unwrap();
    assert_eq!(history.selected, key(SHARED, "POLICY"));
    assert_eq!(history.effective_terminal, key(ROOT, "ROOT-POLICY"));
    assert_eq!(
        graph
            .effective()
            .map(|item| item.key.clone())
            .collect::<Vec<_>>(),
        vec![
            key(ROOT, "ROOT-ADR"),
            key(ROOT, "ROOT-POLICY"),
            key(LEFT, "LEFT-ADR"),
        ]
    );

    let owners = graph
        .ordered_overrides()
        .iter()
        .map(|mapping| mapping.owner_source.as_str())
        .collect::<Vec<_>>();
    assert_eq!(owners, vec![LEFT, ROOT, ROOT]);
    let provenance = graph.provenance_for(&key(ROOT, "ROOT-POLICY")).unwrap();
    assert_eq!(provenance.len(), 3);
    assert_eq!(provenance[0].owner_source, LEFT);
    assert_eq!(provenance[0].state, GraphOverrideState::Lineage);
    assert!(provenance[1..]
        .iter()
        .all(|entry| entry.state == GraphOverrideState::Replacement));

    let intermediate = graph.provenance_for(&key(LEFT, "LEFT-POLICY")).unwrap();
    assert_eq!(
        intermediate
            .iter()
            .map(|entry| entry.state)
            .collect::<Vec<_>>(),
        vec![
            GraphOverrideState::Replacement,
            GraphOverrideState::Overridden
        ]
    );
}

#[test]
fn parent_authored_relationship_keeps_history_and_root_redirects_only_live_endpoint() {
    let graph = GraphComposition::compose(GraphCompositionInput::new(
        ROOT,
        diamond_nodes(),
        vec![
            item(SHARED, "policy.md", "POLICY", "requirement", "Accepted", ""),
            item(
                SHARED,
                "consumer.md",
                "CONSUMER",
                "requirement",
                "Accepted",
                "## Related Requirements\n\n- POLICY\n",
            ),
            item(
                ROOT,
                "replacement.md",
                "ROOT-POLICY",
                "requirement",
                "Accepted",
                "",
            ),
            item(ROOT, "rationale.md", "ROOT-ADR", "decision", "Accepted", ""),
        ],
        vec![mapping(ROOT, (SHARED, "POLICY"), "ROOT-POLICY", "ROOT-ADR")],
    ))
    .unwrap();

    let catalog = graph
        .catalog_relationships()
        .into_iter()
        .find(|relationship| relationship.source == key(SHARED, "CONSUMER"))
        .unwrap();
    assert_eq!(catalog.authored_token, "POLICY");
    assert_eq!(catalog.historical_candidates, vec![key(SHARED, "POLICY")]);
    assert_eq!(catalog.effective_terminal, Some(key(SHARED, "POLICY")));

    let effective = graph
        .effective_relationships()
        .into_iter()
        .find(|relationship| relationship.source == key(SHARED, "CONSUMER"))
        .unwrap();
    assert_eq!(
        effective.historical_candidates,
        catalog.historical_candidates
    );
    assert_eq!(effective.effective_terminal, Some(key(ROOT, "ROOT-POLICY")));
    assert_eq!(effective.issue, None);

    let compatible = graph
        .relationships()
        .into_iter()
        .find(|relationship| {
            relationship
                .source_artifact
                .as_ref()
                .is_some_and(|path| path.source == SHARED && path.relative_path == "consumer.md")
        })
        .unwrap();
    assert_eq!(
        compatible.resolved_artifact.unwrap().source,
        ROOT,
        "existing consumers receive the root-effective endpoint"
    );
    let replacement = graph
        .effective_index()
        .into_iter()
        .find(|entry| entry.key.as_ref() == Some(&key(ROOT, "ROOT-POLICY")))
        .unwrap();
    assert_eq!(replacement.inbound_count, 1);
}

#[test]
fn topology_and_input_order_do_not_change_semantic_ordering() {
    let items = vec![
        item(SHARED, "policy.md", "POLICY", "requirement", "Accepted", ""),
        item(
            ROOT,
            "replacement.md",
            "ROOT-POLICY",
            "requirement",
            "Accepted",
            "",
        ),
        item(ROOT, "rationale.md", "ROOT-ADR", "decision", "Accepted", ""),
    ];
    let first = GraphComposition::compose(GraphCompositionInput::new(
        ROOT,
        diamond_nodes(),
        items.clone(),
        vec![mapping(ROOT, (SHARED, "POLICY"), "ROOT-POLICY", "ROOT-ADR")],
    ))
    .unwrap();
    let mut nodes = diamond_nodes();
    nodes.reverse();
    nodes
        .iter_mut()
        .for_each(|node| node.direct_parents.reverse());
    let second = GraphComposition::compose(GraphCompositionInput::new(
        ROOT,
        nodes,
        items.into_iter().rev().collect(),
        vec![mapping(ROOT, (SHARED, "POLICY"), "ROOT-POLICY", "ROOT-ADR")],
    ))
    .unwrap();

    assert_eq!(first.topological_sources(), second.topological_sources());
    assert_eq!(first.ordered_overrides(), second.ordered_overrides());
    assert_eq!(first.terminal_redirects(), second.terminal_redirects());
    assert_eq!(
        first
            .effective()
            .map(|item| item.key.clone())
            .collect::<Vec<_>>(),
        second
            .effective()
            .map(|item| item.key.clone())
            .collect::<Vec<_>>()
    );
}

#[test]
fn graph_cycle_fails_before_any_projection_is_published() {
    let result = GraphComposition::compose(GraphCompositionInput::new(
        ROOT,
        vec![
            node(ROOT, vec![parent(LEFT, "left")]),
            node(LEFT, vec![parent(ROOT, "root")]),
        ],
        Vec::new(),
        Vec::new(),
    ));
    let findings = match result {
        Ok(_) => panic!("cyclic graph must fail"),
        Err(findings) => findings,
    };
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].reason, GraphFindingReason::Cycle);
}

#[test]
fn edge_alias_validation_is_rechecked_at_the_semantic_boundary() {
    let result = GraphComposition::compose(GraphCompositionInput::new(
        ROOT,
        vec![
            node(ROOT, vec![parent(SHARED, "Bad/Alias")]),
            node(SHARED, Vec::new()),
        ],
        Vec::new(),
        Vec::new(),
    ));
    let findings = match result {
        Ok(_) => panic!("invalid alias must fail"),
        Err(findings) => findings,
    };
    assert!(findings
        .iter()
        .any(|finding| finding.reason == GraphFindingReason::InvalidAlias));
}

#[test]
fn qualified_legacy_alias_is_rejected_even_when_bare_alias_resolves() {
    let text = "---\nschema_version: 1\nid: REQ-KWJ4VMKVSS66\ntype: requirement\n---\n# Qualified fixture\n\n## ID\n\nlegacy-name\n\n## Status\n\nAccepted\n\n## Problem\n\nAliases are not canonical.\n\n## Requirements\n\n- [REQ-001] Qualified references MUST be canonical.\n";
    let origin = CorpusLayer::inherited(SHARED, "runtime-only", "sha256-v2:fixture").origin();
    let display = "/runtime/acme/shared/qualified.md";
    let artifact = CorpusItem::new(
        display.to_string(),
        "qualified.md".to_string(),
        parse_text(text, display),
        spec_for("requirement"),
        origin,
        PhysicalArtifactLocator::new(
            PhysicalCorpusLocator::new("/runtime/acme/shared", "/runtime/acme/shared/decisions"),
            display,
        ),
    );
    let graph = GraphComposition::compose(GraphCompositionInput::new(
        ROOT,
        vec![
            node(ROOT, vec![parent(SHARED, "shared")]),
            node(SHARED, Vec::new()),
        ],
        vec![artifact],
        Vec::new(),
    ))
    .unwrap();

    assert_eq!(
        graph.resolve_public("legacy-name").unwrap().selected,
        key(SHARED, "REQ-KWJ4VMKVSS66")
    );
    assert_eq!(
        graph.resolve_public("acme/shared::legacy-name"),
        Err(GraphLookupError::QualifiedCanonicalRequired)
    );
    assert_eq!(
        graph.resolve_public("shared::legacy-name"),
        Err(GraphLookupError::QualifiedCanonicalRequired)
    );
}

#[test]
fn self_relationship_remains_an_explicit_issue() {
    let graph = GraphComposition::compose(GraphCompositionInput::new(
        ROOT,
        vec![node(ROOT, Vec::new())],
        vec![item(
            ROOT,
            "self.md",
            "SELF",
            "requirement",
            "Accepted",
            "## Related Requirements\n\n- SELF\n",
        )],
        Vec::new(),
    ))
    .unwrap();
    let relationship = graph.effective_relationships().pop().unwrap();
    assert_eq!(
        relationship.issue,
        Some(GraphRelationshipIssue::SelfReference)
    );
    assert_eq!(relationship.effective_terminal, None);
    let summary = graph.relationship_summary();
    assert_eq!(summary.total, 1);
    assert_eq!(summary.valid, 0);
    assert_eq!(summary.broken, 1);
    assert_eq!(summary.issues.len(), 1);
}

#[test]
fn root_is_the_only_writable_projection() {
    let graph = GraphComposition::compose(GraphCompositionInput::new(
        ROOT,
        vec![
            node(ROOT, vec![parent(SHARED, "shared")]),
            node(SHARED, Vec::new()),
        ],
        vec![
            item(ROOT, "root.md", "ROOT-REQ", "requirement", "Accepted", ""),
            item(
                SHARED,
                "shared.md",
                "SHARED-REQ",
                "requirement",
                "Accepted",
                "",
            ),
        ],
        Vec::new(),
    ))
    .unwrap();
    assert_eq!(graph.catalog().len(), 2);
    assert_eq!(graph.effective().len(), 2);
    assert_eq!(graph.root_local().len(), 1);
    assert_eq!(
        graph.root_local().next().unwrap().origin.layer,
        Layer::Local
    );
    assert_eq!(graph.effective_from(SHARED).unwrap().len(), 1);
    assert_eq!(
        graph.resolve_identity("SHARED-REQ").outcome,
        OUTCOME_RESOLVED
    );
}
