use rac_engine::federated_corpus::{is_read_only_materialised_path, load_composed_corpus};
use rac_engine::federation::{
    calculate_parent_digest, direct_graph_materialisation_roots, load_manifest, verify_parent,
    ParentCorpusErrorCode,
};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

static COUNTER: AtomicUsize = AtomicUsize::new(0);
const V2_ZERO_DIGEST: &str =
    "sha256-v2:0000000000000000000000000000000000000000000000000000000000000000";

fn scratch(name: &str) -> PathBuf {
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let path = std::env::temp_dir().join(format!(
        "asdecided-federation-integration-{name}-{}-{n}",
        std::process::id()
    ));
    fs::create_dir_all(&path).unwrap();
    path
}

fn parent_at(root: &Path, source: &str) {
    fs::create_dir_all(root.join(".decided")).unwrap();
    fs::create_dir_all(root.join("decisions/nested")).unwrap();
    fs::write(
        root.join(".decided/config.yaml"),
        format!("repository_key: STD\ncorpus:\n  source: {source}\n"),
    )
    .unwrap();
    fs::write(root.join("decisions/one.md"), b"one\n").unwrap();
    fs::write(root.join("decisions/nested/two.md"), b"two\n").unwrap();
}

fn child_manifest(root: &Path, source: &str, pin: &str, parent_source: &str) {
    fs::create_dir_all(root.join(".decided")).unwrap();
    fs::write(
        root.join(".decided/config.yaml"),
        format!("repository_key: APP\ncorpus:\n  source: {source}\n"),
    )
    .unwrap();
    fs::write(
        root.join(".decided/corpus.md"),
        format!(
            "# Corpus\n\n## inherits\n\n```yaml\nversion: 1\nalias: standards\nsource: {parent_source}\nroot: vendor/standards\ncorpus: decisions\ndigest: {pin}\n```\n\n## overrides\n\n```yaml\nversion: 1\nitems: []\n```\n"
        ),
    )
    .unwrap();
}

fn graph_manifest(root: &Path, source: &str, parents: &[(&str, &str, &str)]) {
    fs::create_dir_all(root.join(".decided")).unwrap();
    fs::write(
        root.join(".decided/config.yaml"),
        format!("repository_key: APP\ncorpus:\n  source: {source}\n"),
    )
    .unwrap();
    let mut manifest = String::from("# Corpus\n\n## inherits\n\n```yaml\nversion: 2\nparents:\n");
    for (alias, parent_source, parent_root) in parents {
        manifest.push_str(&format!(
            "  - alias: {alias}\n    source: {parent_source}\n    root: {parent_root}\n    corpus: decisions\n    digest: {V2_ZERO_DIGEST}\n"
        ));
    }
    manifest.push_str("```\n\n## overrides\n\n```yaml\nversion: 2\nitems: []\n```\n");
    fs::write(root.join(".decided/corpus.md"), manifest).unwrap();
}

#[test]
fn nearest_config_starts_an_independent_repository_beneath_an_outer_graph() {
    let outer = scratch("nested-independent-repository");
    graph_manifest(
        &outer,
        "acme/outer",
        &[("missing", "acme/missing", "vendor/missing")],
    );

    let nested = outer.join("tools/independent");
    fs::create_dir_all(nested.join(".decided")).unwrap();
    fs::create_dir_all(nested.join("decisions")).unwrap();
    fs::write(
        nested.join(".decided/config.yaml"),
        b"repository_key: IND\ncorpus:\n  source: acme/independent\n",
    )
    .unwrap();

    assert!(
        load_composed_corpus(nested.join("decisions").to_str().unwrap(), true)
            .unwrap()
            .is_none(),
        "the outer graph manifest must not govern a nearer configured repository"
    );
    fs::remove_dir_all(outer).unwrap();
}

#[test]
fn every_direct_v2_parent_and_missing_suffix_is_read_only() {
    let child = scratch("v2-three-parents");
    for (root, source) in [
        ("vendor/one", "acme/one"),
        ("vendor/two", "acme/two"),
        ("vendor/three", "acme/three"),
    ] {
        parent_at(&child.join(root), source);
    }
    graph_manifest(
        &child,
        "acme/app",
        &[
            ("one", "acme/one", "vendor/one"),
            ("two", "acme/two", "vendor/two"),
            ("three", "acme/three", "vendor/three"),
        ],
    );

    let roots = direct_graph_materialisation_roots(&child).unwrap().unwrap();
    assert_eq!(roots.len(), 3);
    for target in [
        child.join("vendor/one/decisions/new.md"),
        child.join("vendor/two/new.html"),
        child.join("vendor/three/new-okf/nested/file.json"),
    ] {
        assert!(
            is_read_only_materialised_path(&target).unwrap(),
            "{target:?}"
        );
    }
    fs::remove_dir_all(child).unwrap();
}

#[test]
fn transitive_node_local_and_nested_parent_paths_stay_read_only() {
    let child = scratch("v2-transitive");
    let branch = child.join("vendor/branch");
    let leaf = branch.join("vendor/leaf");
    parent_at(&branch, "acme/branch");
    parent_at(&leaf, "acme/leaf");
    graph_manifest(
        &branch,
        "acme/branch",
        &[("leaf", "acme/leaf", "vendor/leaf")],
    );
    graph_manifest(
        &child,
        "acme/app",
        &[("branch", "acme/branch", "vendor/branch")],
    );

    for target in [
        branch.join("decisions/branch-new.md"),
        leaf.join("decisions/leaf-new.md"),
        leaf.join("not-created/deep/output.html"),
    ] {
        assert!(
            is_read_only_materialised_path(&target).unwrap(),
            "{target:?}"
        );
    }
    fs::remove_dir_all(child).unwrap();
}

#[test]
fn v2_siblings_parent_paths_and_traversal_do_not_cross_the_boundary() {
    let child = scratch("v2-sibling-boundary");
    let parent = child.join("vendor/standards");
    parent_at(&parent, "acme/standards");
    fs::create_dir_all(child.join("vendor/sibling")).unwrap();
    fs::create_dir_all(child.join("decisions")).unwrap();
    graph_manifest(
        &child,
        "acme/app",
        &[("standards", "acme/standards", "vendor/standards")],
    );

    assert!(
        is_read_only_materialised_path(child.join("vendor/sibling/../standards/new.html")).unwrap()
    );
    for target in [
        child.join("vendor/sibling/new.html"),
        child.join("decisions/new.md"),
        child.join("../outside-new.md"),
    ] {
        assert!(
            !is_read_only_materialised_path(&target).unwrap(),
            "{target:?}"
        );
    }
    fs::remove_dir_all(child).unwrap();
}

#[cfg(unix)]
#[test]
fn v2_symlink_ancestor_cannot_disguise_an_inherited_target() {
    use std::os::unix::fs::symlink;

    let child = scratch("v2-symlink-ancestor");
    let parent = child.join("vendor/standards");
    parent_at(&parent, "acme/standards");
    graph_manifest(
        &child,
        "acme/app",
        &[("standards", "acme/standards", "vendor/standards")],
    );
    symlink(
        "standards/decisions",
        child.join("vendor/parent-subdir-link"),
    )
    .unwrap();

    let target = child.join("vendor/parent-subdir-link/../new.html");
    assert!(is_read_only_materialised_path(&target).unwrap());
    assert!(!target.exists());

    let outside = scratch("v2-symlink-outside-target");
    symlink(&outside, parent.join("outside-link")).unwrap();
    let outward_target = parent.join("outside-link/forbidden.html");
    assert!(is_read_only_materialised_path(&outward_target).unwrap());
    assert!(!outside.join("forbidden.html").exists());
    fs::remove_dir_all(outside).unwrap();
    fs::remove_dir_all(child).unwrap();
}

#[test]
fn malformed_v2_manifest_fails_closed_instead_of_using_v1_verification() {
    let child = scratch("v2-malformed-write-boundary");
    parent_at(&child.join("vendor/one"), "acme/one");
    parent_at(&child.join("vendor/two"), "acme/two");
    graph_manifest(
        &child,
        "acme/app",
        &[
            ("same", "acme/one", "vendor/one"),
            ("same", "acme/two", "vendor/two"),
        ],
    );

    let error = is_read_only_materialised_path(child.join("decisions/new.md")).unwrap_err();
    assert_eq!(error.stable_code(), "corpus-federation-duplicate-parent");
    fs::remove_dir_all(child).unwrap();

    let directory_manifest = scratch("v2-directory-manifest-write-boundary");
    fs::create_dir_all(directory_manifest.join(".decided/corpus.md")).unwrap();
    let error = is_read_only_materialised_path(directory_manifest.join("new.md")).unwrap_err();
    assert_eq!(error.stable_code(), "parent-corpus-symlink-traversal");
    fs::remove_dir_all(directory_manifest).unwrap();
}

#[test]
fn no_manifest_and_v1_write_boundary_behaviour_is_unchanged() {
    let plain = scratch("write-boundary-no-manifest");
    fs::create_dir_all(plain.join("decisions")).unwrap();
    assert!(!is_read_only_materialised_path(plain.join("decisions/new.md")).unwrap());
    fs::remove_dir_all(plain).unwrap();

    let child = scratch("write-boundary-v1-parity");
    let parent = child.join("vendor/standards");
    parent_at(&parent, "acme/standards");
    let pin = calculate_parent_digest(&parent, "decisions")
        .unwrap()
        .digest;
    child_manifest(&child, "acme/app", &pin, "acme/standards");
    assert!(is_read_only_materialised_path(parent.join("decisions/new.md")).unwrap());
    assert!(!is_read_only_materialised_path(child.join("local-new.md")).unwrap());
    fs::remove_dir_all(child).unwrap();
}

#[test]
fn nonexistent_traversal_targets_resolve_inside_the_read_only_parent() {
    let child = scratch("write-target-traversal");
    let parent = child.join("vendor/standards");
    parent_at(&parent, "acme/standards");
    let pin = calculate_parent_digest(&parent, "decisions")
        .unwrap()
        .digest;
    child_manifest(&child, "acme/app", &pin, "acme/standards");
    fs::create_dir_all(child.join("vendor/sibling")).unwrap();

    for target in [
        child.join("vendor/sibling/../standards/new.html"),
        child.join("vendor/sibling/../standards/new-okf"),
    ] {
        assert!(is_read_only_materialised_path(&target).unwrap(), "{target:?}");
        assert!(!target.exists());
    }
    fs::remove_dir_all(child).unwrap();
}

#[cfg(unix)]
#[test]
fn symlinked_parent_subdir_then_parent_component_stays_read_only() {
    use std::os::unix::fs::symlink;

    let child = scratch("write-target-symlink-parent");
    let parent = child.join("vendor/standards");
    parent_at(&parent, "acme/standards");
    let pin = calculate_parent_digest(&parent, "decisions")
        .unwrap()
        .digest;
    child_manifest(&child, "acme/app", &pin, "acme/standards");
    symlink(
        "standards/decisions",
        child.join("vendor/parent-subdir-link"),
    )
    .unwrap();

    for target in [
        child.join("vendor/parent-subdir-link/../new.html"),
        child.join("vendor/parent-subdir-link/../new-okf"),
    ] {
        assert!(is_read_only_materialised_path(&target).unwrap(), "{target:?}");
        assert!(!target.exists());
    }
    fs::remove_dir_all(child).unwrap();
}

#[test]
fn clone_location_and_metadata_do_not_change_the_pin() {
    let first = scratch("clone-a");
    let second = scratch("clone-b");
    parent_at(&first, "acme/standards");
    parent_at(&second, "acme/standards");

    let first_pin = calculate_parent_digest(&first, "decisions").unwrap().digest;
    let second_pin = calculate_parent_digest(&second, "decisions")
        .unwrap()
        .digest;
    assert_eq!(first_pin, second_pin);

    fs::remove_dir_all(first).unwrap();
    fs::remove_dir_all(second).unwrap();
}

#[test]
fn config_and_markdown_bytes_are_both_pin_inputs() {
    let root = scratch("pin-inputs");
    parent_at(&root, "acme/standards");
    let original = calculate_parent_digest(&root, "decisions").unwrap().digest;

    fs::write(root.join("decisions/one.md"), b"ONE\n").unwrap();
    let content_changed = calculate_parent_digest(&root, "decisions").unwrap().digest;
    assert_ne!(original, content_changed);

    fs::write(root.join("decisions/one.md"), b"one\n").unwrap();
    fs::write(
        root.join(".decided/config.yaml"),
        b"repository_key: STD\ncorpus:\n  source: acme/standards\n# reviewed\n",
    )
    .unwrap();
    let config_changed = calculate_parent_digest(&root, "decisions").unwrap().digest;
    assert_ne!(original, config_changed);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn missing_materialisation_and_missing_corpus_are_distinct() {
    let child = scratch("missing-materialisation");
    fs::create_dir_all(child.join(".decided")).unwrap();
    fs::write(
        child.join(".decided/config.yaml"),
        b"repository_key: APP\ncorpus:\n  source: acme/app\n",
    )
    .unwrap();
    child_manifest(
        &child,
        "acme/app",
        "sha256:0000000000000000000000000000000000000000000000000000000000000000",
        "acme/standards",
    );
    assert_eq!(
        verify_parent(&child).unwrap_err().code,
        ParentCorpusErrorCode::MaterialisationMissing
    );

    let parent = child.join("vendor/standards");
    parent_at(&parent, "acme/standards");
    fs::remove_dir_all(parent.join("decisions")).unwrap();
    assert_eq!(
        verify_parent(&child).unwrap_err().code,
        ParentCorpusErrorCode::ParentCorpusMissing
    );
    fs::remove_dir_all(child).unwrap();
}

#[test]
fn source_mismatch_and_source_collision_are_distinct() {
    let child = scratch("sources");
    let parent = child.join("vendor/standards");
    parent_at(&parent, "acme/standards");
    let pin = calculate_parent_digest(&parent, "decisions")
        .unwrap()
        .digest;

    child_manifest(&child, "acme/app", &pin, "acme/other");
    assert_eq!(
        verify_parent(&child).unwrap_err().code,
        ParentCorpusErrorCode::SourceMismatch
    );

    child_manifest(&child, "acme/standards", &pin, "acme/standards");
    assert_eq!(
        verify_parent(&child).unwrap_err().code,
        ParentCorpusErrorCode::SourceCollision
    );
    fs::remove_dir_all(child).unwrap();
}

#[test]
fn absolute_and_parent_relative_manifest_paths_never_load() {
    let child = scratch("unsafe-paths");
    fs::create_dir_all(child.join(".decided")).unwrap();
    let manifest = |root: &str, corpus: &str| {
        format!(
            "# Corpus\n\n## inherits\n\n```yaml\nversion: 1\nalias: standards\nsource: acme/standards\nroot: {root}\ncorpus: {corpus}\ndigest: sha256:0000000000000000000000000000000000000000000000000000000000000000\n```\n"
        )
    };
    fs::write(
        child.join(".decided/corpus.md"),
        manifest("/tmp/parent", "decisions"),
    )
    .unwrap();
    assert_eq!(
        load_manifest(&child).unwrap_err().code,
        ParentCorpusErrorCode::PathEscape
    );
    fs::write(
        child.join(".decided/corpus.md"),
        manifest("vendor/standards", "../decisions"),
    )
    .unwrap();
    assert_eq!(
        load_manifest(&child).unwrap_err().code,
        ParentCorpusErrorCode::PathEscape
    );
    fs::remove_dir_all(child).unwrap();
}

#[test]
fn operational_headings_in_examples_do_not_declare_a_parent() {
    let root = scratch("quoted-heading");
    fs::create_dir_all(root.join(".decided")).unwrap();
    fs::write(
        root.join(".decided/corpus.md"),
        "# Corpus\n\n```markdown\n## inherits\n```\n\n> ## inherits\n",
    )
    .unwrap();
    let error = load_manifest(&root).unwrap_err();
    assert_eq!(error.code, ParentCorpusErrorCode::MalformedManifest);
    assert!(error.message.contains("missing exact lowercase"));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn nested_headings_do_not_end_the_inherits_section() {
    let root = scratch("nested-heading");
    fs::create_dir_all(root.join(".decided")).unwrap();
    fs::write(
        root.join(".decided/corpus.md"),
        "# Corpus\n\n## inherits\n\n> ## Example\n\n```yaml\nversion: 1\nalias: standards\nsource: acme/standards\nroot: vendor/standards\ncorpus: decisions\ndigest: sha256:0000000000000000000000000000000000000000000000000000000000000000\n```\n",
    )
    .unwrap();
    let manifest = load_manifest(&root).unwrap().unwrap();
    assert_eq!(manifest.inherits.alias, "standards");
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_verified_result_retains_manifest_and_snapshot_bytes() {
    let child = scratch("retained-bytes");
    let parent = child.join("vendor/standards");
    parent_at(&parent, "acme/standards");
    let calculated = calculate_parent_digest(&parent, "decisions").unwrap();
    child_manifest(&child, "acme/app", &calculated.digest, "acme/standards");

    let verified = verify_parent(&child).unwrap().unwrap();
    assert_eq!(
        verified.child_config_bytes,
        fs::read(child.join(".decided/config.yaml")).unwrap()
    );
    assert_eq!(verified.config_bytes, calculated.config_bytes);
    assert_eq!(verified.files, calculated.files);
    assert_eq!(
        verified
            .files
            .iter()
            .find(|file| file.relative_path == "one.md")
            .unwrap()
            .bytes,
        b"one\n"
    );
    assert!(verified.overrides.is_some());
    assert!(verified.override_mapping_bytes.is_some());
    assert_eq!(
        verified.manifest_bytes,
        fs::read(child.join(".decided/corpus.md")).unwrap()
    );
    fs::remove_dir_all(child).unwrap();
}

#[cfg(unix)]
#[test]
fn symlink_in_the_declared_corpus_path_is_rejected() {
    use std::os::unix::fs::symlink;

    let child = scratch("corpus-symlink");
    let parent = child.join("vendor/standards");
    fs::create_dir_all(parent.join(".decided")).unwrap();
    fs::create_dir_all(parent.join("real-decisions")).unwrap();
    fs::write(
        parent.join(".decided/config.yaml"),
        b"repository_key: STD\ncorpus:\n  source: acme/standards\n",
    )
    .unwrap();
    symlink("real-decisions", parent.join("decisions")).unwrap();

    let error = calculate_parent_digest(&parent, "decisions").unwrap_err();
    assert_eq!(error.code, ParentCorpusErrorCode::SymlinkTraversal);
    fs::remove_dir_all(child).unwrap();
}

#[cfg(unix)]
#[test]
fn symlinked_parent_config_is_rejected_before_reading_bytes() {
    use std::os::unix::fs::symlink;

    let root = scratch("config-symlink");
    fs::create_dir_all(root.join(".decided")).unwrap();
    fs::create_dir_all(root.join("decisions")).unwrap();
    fs::write(
        root.join("outside-config.yaml"),
        b"repository_key: STD\ncorpus:\n  source: acme/standards\n",
    )
    .unwrap();
    symlink("../outside-config.yaml", root.join(".decided/config.yaml")).unwrap();

    let error = calculate_parent_digest(&root, "decisions").unwrap_err();
    assert_eq!(error.code, ParentCorpusErrorCode::SymlinkTraversal);
    fs::remove_dir_all(root).unwrap();
}
