use rac_engine::revisions::{
    materialize_revision, MissingPathPolicy, RevisionSnapshot, RevisionSnapshotError,
};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

struct Scratch(PathBuf);

impl Scratch {
    fn new(label: &str) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "decided-revision-test-{label}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn git(repo: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(repo)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn init(repo: &Path) {
    git(repo, &["init", "-q"]);
    git(repo, &["config", "user.name", "Revision Test"]);
    git(repo, &["config", "user.email", "revision@example.invalid"]);
}

fn commit_all(repo: &Path, message: &str) -> String {
    git(repo, &["add", "-A"]);
    git(repo, &["commit", "-q", "-m", message]);
    git(repo, &["rev-parse", "HEAD"])
}

#[test]
fn root_corpus_materializes_only_exact_markdown_blobs() {
    let scratch = Scratch::new("root-corpus");
    let repo = scratch.path();
    init(repo);
    std::fs::create_dir_all(repo.join("nested")).unwrap();
    std::fs::create_dir_all(repo.join(".hidden")).unwrap();
    std::fs::write(repo.join("root.md"), "$Format:%H$\n").unwrap();
    std::fs::write(repo.join("nested/keep.md"), "kept despite export-ignore\n").unwrap();
    std::fs::write(repo.join("nested/ignore.txt"), "not corpus input\n").unwrap();
    std::fs::write(repo.join(".hidden/skip.md"), "hidden\n").unwrap();
    std::fs::write(
        repo.join(".gitattributes"),
        "root.md export-subst\nnested/keep.md export-ignore\n",
    )
    .unwrap();
    let revision = commit_all(repo, "fixture");

    let mut snapshot = RevisionSnapshot::open(repo.to_str().unwrap(), &revision).unwrap();
    let corpus = snapshot.materialize_corpus(".").unwrap();
    assert!(corpus.existed);
    assert_eq!(
        std::fs::read_to_string(corpus.path.join("root.md")).unwrap(),
        "$Format:%H$\n"
    );
    assert_eq!(
        std::fs::read_to_string(corpus.path.join("nested/keep.md")).unwrap(),
        "kept despite export-ignore\n"
    );
    assert!(!corpus.path.join("nested/ignore.txt").exists());
    assert!(!corpus.path.join(".hidden").exists());
    assert!(!corpus.path.join(".gitattributes").exists());
}

#[test]
fn missing_policies_distinguish_optional_file_and_empty_corpus() {
    let scratch = Scratch::new("missing");
    let repo = scratch.path();
    init(repo);
    std::fs::write(repo.join("README.md"), "fixture\n").unwrap();
    let revision = commit_all(repo, "fixture");

    let mut snapshot = RevisionSnapshot::open(repo.to_str().unwrap(), &revision).unwrap();
    let optional = snapshot
        .materialize_path(".decided/config.yaml", MissingPathPolicy::Ignore)
        .unwrap();
    assert!(!optional.existed);
    assert!(!optional.path.exists());

    let empty = snapshot.materialize_corpus("decisions").unwrap();
    assert!(!empty.existed);
    assert!(empty.path.is_dir());

    let required = snapshot
        .materialize_corpus_with_policy("parents/missing", MissingPathPolicy::Error)
        .unwrap_err();
    assert!(required.message().contains("path does not exist"));
    assert!(!snapshot.root().join("parents/missing").exists());
}

#[test]
fn replacement_refs_cannot_change_selected_revision_bytes() {
    let scratch = Scratch::new("replace-ref");
    let repo = scratch.path();
    init(repo);
    std::fs::create_dir(repo.join("decisions")).unwrap();
    std::fs::write(repo.join("decisions/a.md"), "original\n").unwrap();
    let original = commit_all(repo, "original");
    std::fs::write(repo.join("decisions/a.md"), "replacement\n").unwrap();
    let replacement = commit_all(repo, "replacement");
    git(repo, &["replace", &original, &replacement]);
    assert_eq!(
        git(repo, &["show", &format!("{original}:decisions/a.md")]),
        "replacement"
    );

    let mut snapshot = RevisionSnapshot::open(repo.to_str().unwrap(), &original).unwrap();
    let corpus = snapshot.materialize_corpus("decisions").unwrap();
    assert_eq!(
        std::fs::read_to_string(corpus.path.join("a.md")).unwrap(),
        "original\n"
    );
}

#[cfg(unix)]
#[test]
fn snapshot_and_legacy_temp_roots_are_private() {
    use std::os::unix::fs::PermissionsExt;

    let scratch = Scratch::new("private-root");
    let repo = scratch.path();
    init(repo);
    std::fs::write(repo.join("a.md"), "bytes\n").unwrap();
    let revision = commit_all(repo, "fixture");

    let snapshot = RevisionSnapshot::open(repo.to_str().unwrap(), &revision).unwrap();
    assert_eq!(
        std::fs::metadata(snapshot.root())
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
    let legacy = materialize_revision(repo.to_str().unwrap(), &revision, ".").unwrap();
    assert_eq!(
        std::fs::metadata(&legacy.corpus)
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
}

#[cfg(unix)]
#[test]
fn unsafe_committed_tree_component_is_rejected_before_materialization() {
    let scratch = Scratch::new("unsafe-tree-name");
    let repo = scratch.path();
    init(repo);
    std::fs::create_dir(repo.join("decisions")).unwrap();
    std::fs::write(repo.join("decisions/bad\\name.md"), "unsafe\n").unwrap();
    let revision = commit_all(repo, "fixture");

    let mut snapshot = RevisionSnapshot::open(repo.to_str().unwrap(), &revision).unwrap();
    let error = snapshot.materialize_corpus("decisions").unwrap_err();
    assert!(error
        .message()
        .contains("unsafe revision snapshot path component"));
    assert!(!snapshot.root().join("decisions/bad/name.md").exists());
}

#[cfg(unix)]
#[test]
fn symlink_policy_matches_plain_and_v2_corpus_semantics() {
    use std::os::unix::fs::symlink;

    let scratch = Scratch::new("corpus-symlink");
    let repo = scratch.path();
    init(repo);
    std::fs::create_dir_all(repo.join("decisions/.hidden")).unwrap();
    symlink("elsewhere", repo.join("decisions/non-markdown-link")).unwrap();
    symlink("elsewhere", repo.join("decisions/.hidden/link.md")).unwrap();
    let revision = commit_all(repo, "fixture");

    let mut snapshot = RevisionSnapshot::open(repo.to_str().unwrap(), &revision).unwrap();
    let ordinary = snapshot.materialize_corpus("decisions").unwrap();
    assert!(!ordinary.path.join("non-markdown-link").exists());
    assert!(!ordinary.path.join(".hidden").exists());

    let mut v2_snapshot = RevisionSnapshot::open(repo.to_str().unwrap(), &revision).unwrap();
    let error = v2_snapshot
        .materialize_corpus_with_policy_and_exclusions("decisions", MissingPathPolicy::Error, &[])
        .unwrap_err();
    assert!(error.message().contains("committed symlink"));
}

#[cfg(unix)]
#[test]
fn child_corpus_excludes_declared_parent_root_before_validation() {
    use std::os::unix::fs::symlink;

    let scratch = Scratch::new("parent-root-exclusion");
    let repo = scratch.path();
    init(repo);
    std::fs::create_dir_all(repo.join("decisions/vendor/p/policy")).unwrap();
    std::fs::create_dir_all(repo.join("decisions/vendor/p/other")).unwrap();
    std::fs::write(repo.join("decisions/child.md"), "child\n").unwrap();
    std::fs::write(repo.join("decisions/vendor/p/policy/parent.md"), "parent\n").unwrap();
    symlink(
        "elsewhere",
        repo.join("decisions/vendor/p/other/non-markdown-link"),
    )
    .unwrap();
    let revision = commit_all(repo, "fixture");

    let mut snapshot = RevisionSnapshot::open(repo.to_str().unwrap(), &revision).unwrap();
    let child = snapshot
        .materialize_corpus_with_policy_and_exclusions(
            "decisions",
            MissingPathPolicy::Error,
            &[PathBuf::from("decisions/vendor/p")],
        )
        .unwrap();
    assert_eq!(
        std::fs::read_to_string(child.path.join("child.md")).unwrap(),
        "child\n"
    );
    assert!(!child.path.join("vendor/p").exists());

    let parent = snapshot
        .materialize_corpus_with_policy("decisions/vendor/p/policy", MissingPathPolicy::Error)
        .unwrap();
    assert_eq!(
        std::fs::read_to_string(parent.path.join("parent.md")).unwrap(),
        "parent\n"
    );
    assert!(!snapshot
        .root()
        .join("decisions/vendor/p/other/non-markdown-link")
        .exists());

    let mut equality_snapshot = RevisionSnapshot::open(repo.to_str().unwrap(), &revision).unwrap();
    let excluded_self = equality_snapshot
        .materialize_corpus_with_policy_and_exclusions(
            "decisions/vendor/p/policy",
            MissingPathPolicy::Error,
            &[PathBuf::from("decisions/vendor/p/policy")],
        )
        .unwrap();
    assert!(excluded_self.existed);
    assert!(excluded_self.path.is_dir());
    assert!(!excluded_self.path.join("parent.md").exists());
}

#[test]
fn nested_submodule_uses_the_historically_recorded_commit_offline() {
    let scratch = Scratch::new("submodule");
    let leaf = scratch.path().join("leaf-source");
    let parent = scratch.path().join("parent");
    std::fs::create_dir(&leaf).unwrap();
    std::fs::create_dir(&parent).unwrap();
    init(&leaf);
    std::fs::create_dir_all(leaf.join("decisions")).unwrap();
    std::fs::create_dir_all(leaf.join(".decided")).unwrap();
    std::fs::write(leaf.join("decisions/leaf.md"), "historical bytes\n").unwrap();
    std::fs::write(leaf.join(".decided/config.yaml"), "repository_key: LEAF\n").unwrap();
    commit_all(&leaf, "leaf old");

    init(&parent);
    let leaf_arg = leaf.to_str().unwrap();
    let output = Command::new("git")
        .args([
            "-c",
            "protocol.file.allow=always",
            "submodule",
            "add",
            "-q",
            leaf_arg,
            "decisions/vendor/leaf",
        ])
        .current_dir(&parent)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "submodule add failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let historical_parent = commit_all(&parent, "parent old");

    let checkout = parent.join("decisions/vendor/leaf");
    git(&checkout, &["config", "user.name", "Revision Test"]);
    git(
        &checkout,
        &["config", "user.email", "revision@example.invalid"],
    );
    std::fs::write(
        checkout.join("decisions/leaf.md"),
        "working checkout is newer\n",
    )
    .unwrap();
    commit_all(&checkout, "leaf new");
    commit_all(&parent, "parent new pointer");
    let head_before = git(&parent, &["rev-parse", "HEAD"]);
    let status_before = git(&parent, &["status", "--porcelain=v1"]);

    let mut snapshot =
        RevisionSnapshot::open(parent.to_str().unwrap(), &historical_parent).unwrap();
    let corpus = snapshot.materialize_corpus("decisions").unwrap();
    assert_eq!(
        std::fs::read_to_string(corpus.path.join("vendor/leaf/decisions/leaf.md")).unwrap(),
        "historical bytes\n"
    );
    let config = snapshot
        .materialize_path(
            "decisions/vendor/leaf/.decided/config.yaml",
            MissingPathPolicy::Error,
        )
        .unwrap();
    assert_eq!(
        std::fs::read_to_string(config.path).unwrap(),
        "repository_key: LEAF\n"
    );
    assert_eq!(git(&parent, &["rev-parse", "HEAD"]), head_before);
    assert_eq!(git(&parent, &["status", "--porcelain=v1"]), status_before);
    assert_eq!(
        std::fs::read_to_string(checkout.join("decisions/leaf.md")).unwrap(),
        "working checkout is newer\n"
    );
}

#[test]
fn deinitialized_submodule_uses_the_remaining_local_object_database() {
    let scratch = Scratch::new("deinitialized-submodule");
    let leaf = scratch.path().join("leaf-source");
    let parent = scratch.path().join("parent");
    std::fs::create_dir(&leaf).unwrap();
    std::fs::create_dir(&parent).unwrap();
    init(&leaf);
    std::fs::create_dir(leaf.join("decisions")).unwrap();
    std::fs::write(leaf.join("decisions/leaf.md"), "local object bytes\n").unwrap();
    commit_all(&leaf, "leaf");

    init(&parent);
    let output = Command::new("git")
        .args([
            "-c",
            "protocol.file.allow=always",
            "submodule",
            "add",
            "-q",
            "--name",
            "custom-leaf",
            leaf.to_str().unwrap(),
            "vendor/leaf",
        ])
        .current_dir(&parent)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "submodule add failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let revision = commit_all(&parent, "parent");
    git(&parent, &["submodule", "deinit", "-f", "--", "vendor/leaf"]);
    assert!(parent.join(".git/modules/custom-leaf/objects").is_dir());

    let mut snapshot = RevisionSnapshot::open(parent.to_str().unwrap(), &revision).unwrap();
    let corpus = snapshot
        .materialize_corpus("vendor/leaf/decisions")
        .unwrap();
    assert_eq!(
        std::fs::read_to_string(corpus.path.join("leaf.md")).unwrap(),
        "local object bytes\n"
    );
}

#[test]
fn missing_blob_object_fails_closed() {
    let scratch = Scratch::new("missing-object");
    let repo = scratch.path();
    init(repo);
    std::fs::create_dir(repo.join("decisions")).unwrap();
    std::fs::write(repo.join("decisions/a.md"), "bytes\n").unwrap();
    let revision = commit_all(repo, "fixture");
    let object = git(repo, &["rev-parse", "HEAD:decisions/a.md"]);
    let mut snapshot = RevisionSnapshot::open(repo.to_str().unwrap(), &revision).unwrap();
    std::fs::remove_file(
        repo.join(".git/objects")
            .join(&object[..2])
            .join(&object[2..]),
    )
    .unwrap();

    let error = snapshot.materialize_corpus("decisions").unwrap_err();
    assert!(matches!(
        error,
        RevisionSnapshotError::MaterializationFailed(_)
    ));
    assert!(
        error.message().contains("ls-tree")
            || error.message().contains("invalid blob header")
            || error.message().contains("cat-file"),
        "unexpected error: {}",
        error.message()
    );
    assert!(!snapshot.root().join("decisions/a.md").exists());
}
