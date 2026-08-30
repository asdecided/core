use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use rac_engine::corpus::{ArtifactKey, Layer};
use rac_engine::federation::{
    calculate_parent_digest, calculate_parent_digest_v2, verify_federation,
};
use rac_engine::graph_federated_corpus::{compose_verified_federation, GRAPH_CORPUS_INVALID_NODE};

static COUNTER: AtomicUsize = AtomicUsize::new(0);

fn scratch(name: &str) -> PathBuf {
    let sequence = COUNTER.fetch_add(1, Ordering::SeqCst);
    let path = std::env::temp_dir().join(format!(
        "asdecided-graph-semantic-{name}-{}-{sequence}",
        std::process::id()
    ));
    fs::create_dir_all(&path).unwrap();
    path
}

fn initialise_node(root: &Path, source: &str) {
    fs::create_dir_all(root.join(".decided")).unwrap();
    fs::create_dir_all(root.join("decisions")).unwrap();
    fs::write(
        root.join(".decided/config.yaml"),
        format!("repository_key: TST\ncorpus:\n  source: {source}\n"),
    )
    .unwrap();
}

fn decision(id: &str, title: &str) -> String {
    format!(
        "---\nschema_version: 1\nid: {id}\ntype: decision\n---\n# {title}\n\n## Status\n\nAccepted\n\n## Context\n\nThe graph fixture needs a deterministic policy.\n\n## Decision\n\nKeep the verified snapshot authoritative.\n\n## Consequences\n\nEvery semantic record retains its source.\n"
    )
}

fn write_decision(root: &Path, name: &str, id: &str) -> Vec<u8> {
    let body = decision(id, name);
    fs::write(root.join("decisions").join(format!("{name}.md")), &body).unwrap();
    body.into_bytes()
}

fn v2_manifest(
    root: &Path,
    parents: &[(&str, &str, &str, &str)],
    overrides: &[(&str, &str, &str)],
) {
    let mut manifest = String::from("# Corpus\n\n## inherits\n\n```yaml\nversion: 2\nparents:\n");
    for (alias, source, parent_root, digest) in parents {
        manifest.push_str(&format!(
            "  - alias: {alias}\n    source: {source}\n    root: {parent_root}\n    corpus: decisions\n    digest: {digest}\n"
        ));
    }
    manifest.push_str("```\n\n## overrides\n\n```yaml\nversion: 2\nitems:");
    if overrides.is_empty() {
        manifest.push_str(" []\n");
    } else {
        manifest.push('\n');
        for (target, replacement, rationale) in overrides {
            manifest.push_str(&format!(
                "  - target: {target}\n    with: {replacement}\n    rationale: {rationale}\n"
            ));
        }
    }
    manifest.push_str("```\n");
    fs::write(root.join(".decided/corpus.md"), manifest).unwrap();
}

fn v1_manifest(
    root: &Path,
    alias: &str,
    source: &str,
    parent_root: &str,
    digest: &str,
    overrides: &[(&str, &str, &str)],
) {
    let mut manifest = format!(
        "# Corpus\n\n## inherits\n\n```yaml\nversion: 1\nalias: {alias}\nsource: {source}\nroot: {parent_root}\ncorpus: decisions\ndigest: {digest}\n```\n\n## overrides\n\n```yaml\nversion: 1\nitems:"
    );
    if overrides.is_empty() {
        manifest.push_str(" []\n");
    } else {
        manifest.push('\n');
        for (target, replacement, rationale) in overrides {
            manifest.push_str(&format!(
                "  - parent: {target}\n    with: {replacement}\n    rationale: {rationale}\n"
            ));
        }
    }
    manifest.push_str("```\n");
    fs::write(root.join(".decided/corpus.md"), manifest).unwrap();
}

fn key(source: &str, id: &str) -> ArtifactKey {
    ArtifactKey::new(source, id)
}

fn one_parent_graph(name: &str) -> PathBuf {
    let root = scratch(name);
    initialise_node(&root, "acme/root");
    write_decision(&root, "root", "ROOT-KWJ4VMKVSS65");
    let parent = root.join("vendor/standards");
    initialise_node(&parent, "acme/standards");
    write_decision(&parent, "policy", "STD-KWJ4VMKVSS65");
    let pin = calculate_parent_digest_v2(&parent, "decisions")
        .unwrap()
        .digest;
    v2_manifest(
        &root,
        &[("standards", "acme/standards", "vendor/standards", &pin)],
        &[],
    );
    root
}

#[test]
fn composes_multiple_nodes_from_captured_bytes_after_checkout_disappears() {
    let root = scratch("multiple-nodes");
    initialise_node(&root, "acme/root");
    let root_bytes = write_decision(&root, "root", "ROOT-KWJ4VMKVSS65");

    let left = root.join("vendor/left");
    initialise_node(&left, "acme/left");
    write_decision(&left, "left", "LEFT-KWJ4VMKVSS65");
    let left_pin = calculate_parent_digest_v2(&left, "decisions")
        .unwrap()
        .digest;

    let right = root.join("vendor/right");
    initialise_node(&right, "acme/right");
    write_decision(&right, "right", "RIGHT-KWJ4VMKVSS65");
    let right_pin = calculate_parent_digest_v2(&right, "decisions")
        .unwrap()
        .digest;

    v2_manifest(
        &root,
        &[
            ("left", "acme/left", "vendor/left", &left_pin),
            ("right", "acme/right", "vendor/right", &right_pin),
        ],
        &[],
    );
    let verified = verify_federation(&root, "decisions").unwrap().unwrap();
    fs::remove_dir_all(&root).unwrap();

    let graph = compose_verified_federation(verified).unwrap();
    assert_eq!(graph.composition.catalog().len(), 3);
    assert_eq!(graph.composition.effective().len(), 3);
    assert_eq!(
        graph.content(&key("acme/root", "ROOT-KWJ4VMKVSS65")),
        Some(root_bytes.as_slice())
    );
    assert_eq!(graph.read_only_materialisation_roots.len(), 2);
    assert_eq!(graph.read_only_corpus_roots.len(), 2);
    assert_eq!(graph.canonical_layers["acme/root"].layer, Layer::Local);
    assert_eq!(
        graph.canonical_layers["acme/left"].pin.as_deref(),
        Some(left_pin.as_str())
    );
    assert_eq!(graph.canonical_layers["acme/left"].alias, None);
}

#[test]
fn semantic_paths_are_checkout_independent_and_never_absolute() {
    let first_root = one_parent_graph("checkout-a");
    let second_root = one_parent_graph("checkout-b");
    let first = compose_verified_federation(
        verify_federation(&first_root, "decisions")
            .unwrap()
            .unwrap(),
    )
    .unwrap();
    let second = compose_verified_federation(
        verify_federation(&second_root, "decisions")
            .unwrap()
            .unwrap(),
    )
    .unwrap();

    let semantic_paths = |graph: &rac_engine::graph_federated_corpus::VerifiedGraphCorpus| {
        graph
            .composition
            .catalog()
            .map(|item| (item.key.clone(), item.path.clone()))
            .collect::<Vec<_>>()
    };
    let first_paths = semantic_paths(&first);
    assert_eq!(first_paths, semantic_paths(&second));
    assert!(first_paths
        .iter()
        .all(|(_, path)| !Path::new(path).is_absolute()));
    assert!(first_paths
        .iter()
        .all(|(_, path)| !path.contains(first_root.to_string_lossy().as_ref())));
    assert!(first_paths
        .iter()
        .all(|(_, path)| !path.contains(second_root.to_string_lossy().as_ref())));

    fs::remove_dir_all(first_root).unwrap();
    fs::remove_dir_all(second_root).unwrap();
}

#[test]
fn lifts_and_composes_a_root_v2_override() {
    let root = scratch("v2-override");
    initialise_node(&root, "acme/root");
    write_decision(&root, "replacement", "ROOT-KWJ4VMKVSS65");
    write_decision(&root, "rationale", "ROOT-KWJ4VMKVSS66");

    let parent = root.join("vendor/standards");
    initialise_node(&parent, "acme/standards");
    write_decision(&parent, "policy", "STD-KWJ4VMKVSS65");
    let pin = calculate_parent_digest_v2(&parent, "decisions")
        .unwrap()
        .digest;
    v2_manifest(
        &root,
        &[("standards", "acme/standards", "vendor/standards", &pin)],
        &[(
            "acme/standards::STD-KWJ4VMKVSS65",
            "ROOT-KWJ4VMKVSS65",
            "ROOT-KWJ4VMKVSS66",
        )],
    );

    let verified = verify_federation(&root, "decisions").unwrap().unwrap();
    let graph = compose_verified_federation(verified).unwrap();
    assert_eq!(graph.composition.catalog().len(), 3);
    assert_eq!(graph.composition.effective().len(), 2);
    let mapping = &graph.composition.ordered_overrides()[0];
    assert_eq!(mapping.owner_source, "acme/root");
    assert_eq!(mapping.target, key("acme/standards", "STD-KWJ4VMKVSS65"));
    assert_eq!(
        graph.composition.terminal_redirects().get(&mapping.target),
        Some(&key("acme/root", "ROOT-KWJ4VMKVSS65"))
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn lifts_a_valid_nested_v1_override_into_the_graph() {
    let root = scratch("nested-v1");
    initialise_node(&root, "acme/root");
    write_decision(&root, "root", "ROOT-KWJ4VMKVSS65");

    let branch = root.join("vendor/branch");
    initialise_node(&branch, "acme/branch");
    write_decision(&branch, "replacement", "BRANCH-KWJ4VMKVSS65");
    write_decision(&branch, "rationale", "BRANCH-KWJ4VMKVSS66");

    let leaf = branch.join("vendor/leaf");
    initialise_node(&leaf, "acme/leaf");
    write_decision(&leaf, "policy", "LEAF-KWJ4VMKVSS65");
    let leaf_v1_pin = calculate_parent_digest(&leaf, "decisions").unwrap().digest;
    v1_manifest(
        &branch,
        "base",
        "acme/leaf",
        "vendor/leaf",
        &leaf_v1_pin,
        &[(
            "base::LEAF-KWJ4VMKVSS65",
            "BRANCH-KWJ4VMKVSS65",
            "BRANCH-KWJ4VMKVSS66",
        )],
    );
    let branch_pin = calculate_parent_digest_v2(&branch, "decisions")
        .unwrap()
        .digest;
    v2_manifest(
        &root,
        &[("branch", "acme/branch", "vendor/branch", &branch_pin)],
        &[],
    );

    let verified = verify_federation(&root, "decisions").unwrap().unwrap();
    assert_eq!(
        verified.node("acme/branch").unwrap().manifest_version,
        Some(1)
    );
    let graph = compose_verified_federation(verified).unwrap();
    assert_eq!(graph.composition.catalog().len(), 4);
    let mapping = graph
        .composition
        .ordered_overrides()
        .iter()
        .find(|mapping| mapping.owner_source == "acme/branch")
        .unwrap();
    assert_eq!(mapping.target, key("acme/leaf", "LEAF-KWJ4VMKVSS65"));
    assert_eq!(
        mapping.replacement,
        key("acme/branch", "BRANCH-KWJ4VMKVSS65")
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn invalid_parent_artifact_returns_a_stable_sourced_error() {
    let root = scratch("invalid-parent");
    initialise_node(&root, "acme/root");
    write_decision(&root, "root", "ROOT-KWJ4VMKVSS65");

    let parent = root.join("vendor/invalid");
    initialise_node(&parent, "acme/invalid");
    fs::write(
        parent.join("decisions/broken.md"),
        "---\nschema_version: 1\nid: BAD-KWJ4VMKVSS65\ntype: decision\n---\n# Broken\n\n## Status\n\nAccepted\n\n## Context\n\nMissing a required section.\n\n## Decision\n\nRemain invalid.\n",
    )
    .unwrap();
    let pin = calculate_parent_digest_v2(&parent, "decisions")
        .unwrap()
        .digest;
    v2_manifest(
        &root,
        &[("invalid", "acme/invalid", "vendor/invalid", &pin)],
        &[],
    );

    let verified = verify_federation(&root, "decisions").unwrap().unwrap();
    fs::remove_dir_all(&root).unwrap();
    let error = match compose_verified_federation(verified) {
        Ok(_) => panic!("invalid parent artifact must block graph composition"),
        Err(error) => error,
    };
    assert_eq!(error.stable_code(), GRAPH_CORPUS_INVALID_NODE);
    assert_eq!(error.source.as_deref(), Some("acme/invalid"));
    assert_eq!(error.relative_path.as_deref(), Some("broken.md"));
    assert!(error.message.contains("structural error"));
    let origin = error.validation_origin.as_ref().unwrap();
    assert_eq!(origin.source, "acme/invalid");
    assert_eq!(origin.layer, Layer::Inherited);
    assert!(origin.pin.as_deref().is_some_and(|pin| pin.starts_with("sha256-v2:")));
    assert_eq!(
        error.source_route.as_deref().unwrap(),
        &["acme/root", "acme/invalid"]
    );
    assert_eq!(error.route_count.as_deref().copied(), Some(1));
}
