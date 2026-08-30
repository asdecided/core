//! Black-box coverage for point-in-time exports of a committed federation graph.

mod federation_graph_support;

use federation_graph_support::{decision, GraphRepo, ParentEdge, ROOT_ID};
use serde_json::Value;
use std::process::{Command, Output};

const PARENT_ID: &str = "STD-01K000000001";
const PARENT_SOURCE: &str = "acme/standards";

fn run(repo: &GraphRepo, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_decided"))
        .args(args)
        .current_dir(repo.root())
        .env("DECIDED_CACHE_DIR", repo.root().join(".decided/cache"))
        .env("XDG_CACHE_HOME", repo.root().join(".xdg/cache"))
        .env("XDG_CONFIG_HOME", repo.root().join(".xdg/config"))
        .env("XDG_STATE_HOME", repo.root().join(".xdg/state"))
        .output()
        .expect("run decided point-in-time federation contract")
}

fn git(repo: &GraphRepo, args: &[&str]) -> Output {
    Command::new("git")
        .args(args)
        .current_dir(repo.root())
        .output()
        .expect("run Git for point-in-time federation fixture")
}

fn assert_success(output: &Output, context: &str) {
    assert!(
        output.status.success(),
        "{context} failed with {:?}\nstdout:\n{}\nstderr:\n{}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn commit_fixture(repo: &GraphRepo) {
    for (context, args) in [
        ("initialize fixture repository", &["init", "--quiet"][..]),
        ("stage federation fixture", &["add", "--all"][..]),
        (
            "commit federation fixture",
            &[
                "-c",
                "user.name=AsDecided Test",
                "-c",
                "user.email=tests@asdecided.dev",
                "commit",
                "--quiet",
                "--message",
                "test: commit v2 federation fixture",
            ][..],
        ),
    ] {
        assert_success(&git(repo, args), context);
    }

    let tree = git(repo, &["ls-tree", "-r", "--name-only", "HEAD"]);
    assert_success(&tree, "list committed federation tree");
    let paths = String::from_utf8_lossy(&tree.stdout);
    for required in [
        ".decided/config.yaml",
        ".decided/corpus.md",
        "decisions/root.md",
        "vendor/standards/.decided/config.yaml",
        "vendor/standards/decisions/standards.md",
    ] {
        assert!(
            paths.lines().any(|path| path == required),
            "committed federation tree omitted {required}:\n{paths}"
        );
    }
}

fn commit_all(repo: &GraphRepo, message: &str) {
    assert_success(&git(repo, &["add", "--all"]), "stage fixture revision");
    assert_success(
        &git(
            repo,
            &[
                "-c",
                "user.name=AsDecided Test",
                "-c",
                "user.email=tests@asdecided.dev",
                "commit",
                "--quiet",
                "--message",
                message,
            ],
        ),
        "commit fixture revision",
    );
}

fn documents(output: &Output, context: &str) -> Vec<Value> {
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|line| {
            serde_json::from_str(line)
                .unwrap_or_else(|error| panic!("{context} emitted invalid JSONL: {error}: {line}"))
        })
        .collect()
}

#[test]
fn point_in_time_export_preserves_committed_v2_federation_and_local_only_projection() {
    let repo = GraphRepo::new("point-in-time-export");
    repo.create_node("vendor/standards", "STD", PARENT_SOURCE);
    repo.write_node(
        "vendor/standards",
        "decisions/standards.md",
        &decision(PARENT_ID, "Inherited Standards Policy", None),
    );
    let pin = repo.v2_digest("vendor/standards", PARENT_SOURCE);
    repo.write_v2_manifest(
        "",
        &[ParentEdge::new(
            "standards",
            PARENT_SOURCE,
            "vendor/standards",
            pin.clone(),
        )],
        &[],
    );
    commit_fixture(&repo);

    let current = run(&repo, &["export", "decisions", "--documents"]);
    assert_success(&current, "export current v2 federation");
    let historical = run(
        &repo,
        &["export", "decisions", "--documents", "--at", "HEAD"],
    );
    assert_success(&historical, "export committed v2 federation at HEAD");
    assert_eq!(
        historical.stdout, current.stdout,
        "point-in-time documents bytes differ from the same checked-out revision"
    );
    assert_eq!(
        historical.stderr, current.stderr,
        "point-in-time export findings differ from the same checked-out revision"
    );

    let exported = documents(&historical, "historical federation export");
    let inherited = exported
        .iter()
        .find(|document| document["id"].as_str() == Some(PARENT_ID))
        .expect("historical export includes inherited parent content");
    assert!(
        inherited["text"]
            .as_str()
            .is_some_and(|text| text.contains("Inherited Standards Policy")),
        "historical export omitted inherited parent body: {inherited}"
    );
    assert_eq!(
        inherited["metadata"]["source"].as_str(),
        Some(PARENT_SOURCE)
    );
    assert_eq!(
        inherited["metadata"]["provenance"]["source"].as_str(),
        Some(PARENT_SOURCE)
    );
    assert_eq!(
        inherited["metadata"]["provenance"]["layer"].as_str(),
        Some("inherited")
    );
    assert_eq!(
        inherited["metadata"]["provenance"]["pin"].as_str(),
        Some(pin.as_str())
    );

    let local = run(
        &repo,
        &[
            "export",
            "decisions",
            "--documents",
            "--local-only",
            "--at",
            "HEAD",
        ],
    );
    assert_success(&local, "export historical local-only projection");
    let local_documents = documents(&local, "historical local-only export");
    assert_eq!(
        local_documents.len(),
        1,
        "local-only export: {local_documents:?}"
    );
    assert_eq!(local_documents[0]["id"].as_str(), Some(ROOT_ID));
    assert!(
        local_documents
            .iter()
            .all(|document| document["metadata"]["source"].as_str() != Some(PARENT_SOURCE)),
        "local-only historical export retained inherited provenance: {local_documents:?}"
    );
}

#[cfg(unix)]
#[test]
fn historical_child_walk_prunes_nested_parent_roots_before_snapshot_validation() {
    use std::os::unix::fs::symlink;

    let repo = GraphRepo::new("point-in-time-parent-root-pruning");
    repo.create_node("decisions/vendor/standards", "STD", PARENT_SOURCE);
    repo.write_node(
        "decisions/vendor/standards",
        "decisions/standards.md",
        &decision(PARENT_ID, "Inherited Standards Policy", None),
    );
    std::fs::create_dir_all(repo.path("decisions/vendor/standards/other"))
        .expect("create unrelated parent content");
    symlink(
        "elsewhere",
        repo.path("decisions/vendor/standards/other/unrelated.md"),
    )
    .expect("create unrelated committed symlink");
    let pin = repo.v2_digest("decisions/vendor/standards", PARENT_SOURCE);
    repo.write_v2_manifest(
        "",
        &[ParentEdge::new(
            "standards",
            PARENT_SOURCE,
            "decisions/vendor/standards",
            pin,
        )],
        &[],
    );
    assert_success(
        &git(&repo, &["init", "--quiet"]),
        "initialize parent-root pruning repository",
    );
    commit_all(&repo, "test: commit nested parent-root pruning fixture");

    let current = run(&repo, &["export", "decisions", "--documents"]);
    assert_success(&current, "export live graph with unrelated parent content");
    let historical = run(
        &repo,
        &["export", "decisions", "--documents", "--at", "HEAD"],
    );
    assert_success(
        &historical,
        "export historical graph with nested parent root pruning",
    );
    assert_eq!(historical.stdout, current.stdout);
    assert_eq!(historical.stderr, current.stderr);
}

#[test]
fn absent_historical_child_corpus_is_empty_even_when_its_manifest_has_a_parent() {
    let repo = GraphRepo::new("absent-historical-child");
    std::fs::remove_dir_all(repo.path("decisions")).expect("remove the old child corpus path");
    repo.create_node("vendor/standards", "STD", PARENT_SOURCE);
    repo.write_node(
        "vendor/standards",
        "decisions/standards.md",
        &decision(PARENT_ID, "Inherited Standards Policy", None),
    );
    let pin = repo.v2_digest("vendor/standards", PARENT_SOURCE);
    repo.write_v2_manifest(
        "",
        &[ParentEdge::new(
            "standards",
            PARENT_SOURCE,
            "vendor/standards",
            pin,
        )],
        &[],
    );

    assert_success(
        &git(&repo, &["init", "--quiet"]),
        "initialize absent-corpus repository",
    );
    commit_all(&repo, "test: commit revision without child corpus");
    let old_revision = git(&repo, &["rev-parse", "HEAD"]);
    assert_success(&old_revision, "resolve absent-corpus revision");
    let old_revision = String::from_utf8(old_revision.stdout)
        .expect("Git revision is UTF-8")
        .trim()
        .to_string();

    repo.write("decisions/root.md", &decision(ROOT_ID, "Root Policy", None));
    commit_all(&repo, "test: add child corpus");
    let current = run(&repo, &["export", "decisions", "--documents"]);
    assert_success(&current, "export later federation revision");
    assert!(
        current
            .stdout
            .windows(ROOT_ID.len())
            .any(|bytes| bytes == ROOT_ID.as_bytes())
            && current
                .stdout
                .windows(PARENT_ID.len())
                .any(|bytes| bytes == PARENT_ID.as_bytes()),
        "later revision should prove both local and inherited records are exportable: {}",
        String::from_utf8_lossy(&current.stdout)
    );

    let historical = run(
        &repo,
        &["export", "decisions", "--documents", "--at", &old_revision],
    );
    assert_success(
        &historical,
        "export revision where requested child corpus is absent",
    );
    assert!(
        historical.stdout.is_empty(),
        "an absent historical child corpus must not project inherited records: {}",
        String::from_utf8_lossy(&historical.stdout)
    );
    assert!(
        historical.stderr.is_empty(),
        "an absent historical child corpus emitted findings: {}",
        String::from_utf8_lossy(&historical.stderr)
    );
}

#[test]
fn absent_declared_parent_corpus_is_not_fabricated_as_an_empty_snapshot() {
    let repo = GraphRepo::new("missing-historical-parent-corpus");
    repo.create_node("vendor/standards", "STD", PARENT_SOURCE);
    let empty_pin = repo.v2_digest("vendor/standards", PARENT_SOURCE);
    std::fs::remove_dir_all(repo.path("vendor/standards/decisions"))
        .expect("remove declared parent corpus before commit");
    repo.write_v2_manifest(
        "",
        &[ParentEdge::new(
            "standards",
            PARENT_SOURCE,
            "vendor/standards",
            empty_pin,
        )],
        &[],
    );
    assert_success(
        &git(&repo, &["init", "--quiet"]),
        "initialize missing-parent repository",
    );
    commit_all(&repo, "test: commit missing declared parent corpus");

    let historical = run(
        &repo,
        &["export", "decisions", "--documents", "--at", "HEAD"],
    );
    assert_eq!(
        historical.status.code(),
        Some(1),
        "missing parent corpus must be a materialization failure\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&historical.stdout),
        String::from_utf8_lossy(&historical.stderr)
    );
    assert!(historical.stdout.is_empty());
    assert!(
        String::from_utf8_lossy(&historical.stderr).contains("path does not exist at revision"),
        "stderr={}",
        String::from_utf8_lossy(&historical.stderr)
    );
}
