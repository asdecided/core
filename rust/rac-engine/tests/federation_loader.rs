use rac_engine::federation::{
    calculate_parent_digest, load_manifest, verify_parent, ParentCorpusErrorCode,
};
use rac_engine::federated_corpus::is_read_only_materialised_path;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

static COUNTER: AtomicUsize = AtomicUsize::new(0);

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
