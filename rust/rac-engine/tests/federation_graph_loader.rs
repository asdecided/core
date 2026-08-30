use rac_engine::federation::{
    calculate_parent_digest_v2, digest_snapshot_v2, load_graph_manifest, load_manifest,
    verify_federation, ParentCorpusErrorCode, SnapshotFile,
};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

static COUNTER: AtomicUsize = AtomicUsize::new(0);

fn scratch(name: &str) -> PathBuf {
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let path = std::env::temp_dir().join(format!(
        "asdecided-federation-v2-{name}-{}-{n}",
        std::process::id()
    ));
    fs::create_dir_all(&path).unwrap();
    path
}

fn node(root: &Path, source: &str, body: &str) {
    fs::create_dir_all(root.join(".decided")).unwrap();
    fs::create_dir_all(root.join("decisions")).unwrap();
    fs::write(
        root.join(".decided/config.yaml"),
        format!("repository_key: TST\ncorpus:\n  source: {source}\n"),
    )
    .unwrap();
    fs::write(root.join("decisions/policy.md"), body).unwrap();
}

fn v2_manifest(root: &Path, parents: &[(&str, &str, &str, &str)]) {
    let mut manifest = String::from("# Corpus\n\n## inherits\n\n```yaml\nversion: 2\nparents:\n");
    for (alias, source, parent_root, digest) in parents {
        manifest.push_str(&format!(
            "  - alias: {alias}\n    source: {source}\n    root: {parent_root}\n    corpus: decisions\n    digest: {digest}\n"
        ));
    }
    manifest.push_str("```\n\n## overrides\n\n```yaml\nversion: 2\nitems: []\n```\n");
    fs::write(root.join(".decided/corpus.md"), manifest).unwrap();
}

fn diamond(root: &Path, second_shared_body: &str) {
    node(root, "acme/root", "root\n");
    let branch_a = root.join("decisions/vendor/a");
    let branch_b = root.join("decisions/vendor/b");
    let shared_a = branch_a.join("vendor/shared");
    let shared_b = branch_b.join("vendor/shared");
    node(&branch_a, "acme/a", "a\n");
    node(&branch_b, "acme/b", "b\n");
    node(&shared_a, "acme/shared", "shared\n");
    node(&shared_b, "acme/shared", second_shared_body);
    let shared_a_pin = calculate_parent_digest_v2(&shared_a, "decisions")
        .unwrap()
        .digest;
    let shared_b_pin = calculate_parent_digest_v2(&shared_b, "decisions")
        .unwrap()
        .digest;
    v2_manifest(
        &branch_a,
        &[("shared", "acme/shared", "vendor/shared", &shared_a_pin)],
    );
    v2_manifest(
        &branch_b,
        &[("shared", "acme/shared", "vendor/shared", &shared_b_pin)],
    );
    let a_pin = calculate_parent_digest_v2(&branch_a, "decisions")
        .unwrap()
        .digest;
    let b_pin = calculate_parent_digest_v2(&branch_b, "decisions")
        .unwrap()
        .digest;
    v2_manifest(
        root,
        &[
            ("a", "acme/a", "decisions/vendor/a", &a_pin),
            ("b", "acme/b", "decisions/vendor/b", &b_pin),
        ],
    );
}

#[test]
fn digest_v2_has_a_fixed_manifest_presence_vector() {
    let files = vec![SnapshotFile {
        relative_path: "policy.md".to_string(),
        absolute_path: PathBuf::from("ignored"),
        bytes: b"policy\r\n".to_vec(),
    }];
    assert_eq!(
        digest_snapshot_v2(
            "acme/standards",
            b"repository_key: STD\ncorpus:\n  source: acme/standards\n",
            None,
            &files,
        ),
        "sha256-v2:a98e4e89445427bc5a0fdeaffa9bc479895675786ceb987c128db43aad0fa9c1"
    );
    assert_ne!(
        digest_snapshot_v2(
            "acme/standards",
            b"repository_key: STD\ncorpus:\n  source: acme/standards\n",
            Some(b""),
            &files,
        ),
        digest_snapshot_v2(
            "acme/standards",
            b"repository_key: STD\ncorpus:\n  source: acme/standards\n",
            None,
            &files,
        )
    );
}

#[test]
fn verifies_every_diamond_route_then_deduplicates_the_logical_source() {
    let root = scratch("diamond");
    diamond(&root, "shared\n");
    let closure = verify_federation(&root, "decisions").unwrap().unwrap();
    assert_eq!(
        closure
            .nodes
            .iter()
            .map(|node| node.source.as_str())
            .collect::<Vec<_>>(),
        ["acme/a", "acme/b", "acme/shared"]
    );
    assert_eq!(closure.edges.len(), 4);
    assert_eq!(closure.materialisation_roots.len(), 4);
    assert_eq!(closure.node("acme/a").unwrap().manifest_version, Some(2));
    assert!(closure.node("acme/a").unwrap().overrides.is_some());
    assert_eq!(
        closure
            .root_files
            .iter()
            .map(|file| file.relative_path.as_str())
            .collect::<Vec<_>>(),
        ["policy.md"]
    );
    assert!(closure.contains_materialised_path(&root.join("decisions/vendor/a/new.md")));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_tampered_second_diamond_route_fails_instead_of_trusting_the_first_copy() {
    let root = scratch("diamond-tamper");
    diamond(&root, "shared\n");
    fs::write(
        root.join("decisions/vendor/b/vendor/shared/decisions/policy.md"),
        "tampered\n",
    )
    .unwrap();
    assert_eq!(
        verify_federation(&root, "decisions").unwrap_err().code,
        ParentCorpusErrorCode::DigestMismatch
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn same_source_with_distinct_verified_v2_digests_is_a_divergent_pin() {
    let root = scratch("divergent");
    diamond(&root, "different\n");
    assert_eq!(
        verify_federation(&root, "decisions").unwrap_err().code,
        ParentCorpusErrorCode::DivergentPin
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn active_root_source_recurrence_is_a_cycle_after_pin_verification() {
    let root = scratch("cycle");
    node(&root, "acme/root", "root\n");
    let branch = root.join("vendor/branch");
    let recurrence = branch.join("vendor/recurrence");
    node(&branch, "acme/branch", "branch\n");
    node(&recurrence, "acme/root", "nested root\n");
    let recurrence_pin = calculate_parent_digest_v2(&recurrence, "decisions")
        .unwrap()
        .digest;
    v2_manifest(
        &branch,
        &[(
            "root-again",
            "acme/root",
            "vendor/recurrence",
            &recurrence_pin,
        )],
    );
    let branch_pin = calculate_parent_digest_v2(&branch, "decisions")
        .unwrap()
        .digest;
    v2_manifest(
        &root,
        &[("branch", "acme/branch", "vendor/branch", &branch_pin)],
    );
    assert_eq!(
        verify_federation(&root, "decisions").unwrap_err().code,
        ParentCorpusErrorCode::Cycle
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn v2_parser_rejects_duplicate_sources_aliases_and_non_posix_paths() {
    let root = scratch("strict-manifest");
    node(&root, "acme/root", "root\n");
    let digest = "sha256-v2:0000000000000000000000000000000000000000000000000000000000000000";
    v2_manifest(
        &root,
        &[
            ("one", "acme/shared", "vendor/one", digest),
            ("two", "acme/shared", "vendor/two", digest),
        ],
    );
    assert_eq!(
        load_graph_manifest(&root).unwrap_err().code,
        ParentCorpusErrorCode::DuplicateParent
    );
    v2_manifest(&root, &[("one", "acme/shared", "vendor\\one", digest)]);
    assert_eq!(
        load_graph_manifest(&root).unwrap_err().code,
        ParentCorpusErrorCode::PathEscape
    );
    fs::write(
        root.join(".decided/corpus.md"),
        format!(
            "# Corpus\n\n## inherits\n\n```yaml\nversion: 2\nparents:\n  - alias: one\n    alias: two\n    source: acme/shared\n    root: vendor/one\n    corpus: decisions\n    digest: {digest}\n```\n"
        ),
    )
    .unwrap();
    assert_eq!(
        load_graph_manifest(&root).unwrap_err().code,
        ParentCorpusErrorCode::MalformedManifest
    );
    fs::write(
        root.join(".decided/corpus.md"),
        format!(
            "# Corpus\n\n## inherits\n\n```yaml\nversion: 2\nparents:\n  - &parent\n    alias: one\n    source: acme/shared\n    root: vendor/one\n    corpus: decisions\n    digest: {digest}\n```\n"
        ),
    )
    .unwrap();
    assert_eq!(
        load_graph_manifest(&root).unwrap_err().code,
        ParentCorpusErrorCode::MalformedManifest
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn version_one_stays_on_the_existing_loader_path() {
    let root = scratch("v1");
    node(&root, "acme/root", "root\n");
    fs::write(
        root.join(".decided/corpus.md"),
        "# Corpus\n\n## inherits\n\n```yaml\nversion: 1\nalias: standards\nsource: acme/standards\nroot: vendor/standards\ncorpus: decisions\ndigest: sha256:0000000000000000000000000000000000000000000000000000000000000000\n```\n",
    )
    .unwrap();
    assert!(load_manifest(&root).unwrap().is_some());
    assert!(verify_federation(&root, "decisions").unwrap().is_none());
    fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
#[test]
fn inherited_hard_links_fail_closed() {
    let root = scratch("hard-link");
    node(&root, "acme/root", "root\n");
    let parent = root.join("vendor/parent");
    node(&parent, "acme/parent", "parent\n");
    fs::hard_link(
        parent.join("decisions/policy.md"),
        parent.join("decisions/alias.md"),
    )
    .unwrap();
    let pin = digest_snapshot_v2(
        "acme/parent",
        &fs::read(parent.join(".decided/config.yaml")).unwrap(),
        None,
        &[],
    );
    v2_manifest(&root, &[("parent", "acme/parent", "vendor/parent", &pin)]);
    assert_eq!(
        verify_federation(&root, "decisions").unwrap_err().code,
        ParentCorpusErrorCode::UnsupportedFilesystem
    );
    fs::remove_dir_all(root).unwrap();
}
