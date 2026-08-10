//! Black-box guards for graph-aware inspection and inherited write boundaries.

mod federation_graph_support;

use federation_graph_support::{decision, GraphRepo, ParentEdge};
use serde_json::Value;
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::process::{Command, Output};

const PARENT_SOURCE: &str = "acme/runtime-parent";
const PARENT_ID: &str = "PAR-01K000000001";

fn runtime_repo(label: &str) -> GraphRepo {
    let repo = GraphRepo::new(label);
    repo.create_node("vendor/parent", "PAR", PARENT_SOURCE);
    repo.write_node(
        "vendor/parent",
        "decisions/parent.md",
        &decision(PARENT_ID, "Inherited Runtime Policy", None),
    );
    repo.write_v2_manifest(
        "",
        &[ParentEdge::new(
            "parent",
            PARENT_SOURCE,
            "vendor/parent",
            repo.v2_digest("vendor/parent", PARENT_SOURCE),
        )],
        &[],
    );
    repo
}

fn run(repo: &GraphRepo, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_decided"))
        .args(args)
        .current_dir(repo.root())
        .env("DECIDED_NO_CACHE", "1")
        .env("XDG_CACHE_HOME", repo.path(".xdg/cache"))
        .env("XDG_CONFIG_HOME", repo.path(".xdg/config"))
        .env("XDG_STATE_HOME", repo.path(".xdg/state"))
        .output()
        .expect("run decided runtime guard contract")
}

fn assert_read_only_failure(output: &Output, context: &str) {
    assert_eq!(
        output.status.code(),
        Some(1),
        "{context}\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr).to_lowercase();
    assert!(
        stderr.contains("refusing to write") && stderr.contains("read-only"),
        "{context}: {stderr}"
    );
}

fn file_bytes(root: &Path) -> BTreeMap<String, Vec<u8>> {
    fn collect(root: &Path, current: &Path, files: &mut BTreeMap<String, Vec<u8>>) {
        let mut entries = fs::read_dir(current)
            .expect("read snapshot directory")
            .map(|entry| entry.expect("read snapshot entry").path())
            .collect::<Vec<_>>();
        entries.sort();
        for path in entries {
            let metadata = fs::symlink_metadata(&path).expect("snapshot metadata");
            if metadata.is_dir() {
                collect(root, &path, files);
            } else if metadata.is_file() {
                files.insert(
                    path.strip_prefix(root)
                        .expect("snapshot path beneath root")
                        .to_string_lossy()
                        .replace('\\', "/"),
                    fs::read(path).expect("snapshot bytes"),
                );
            }
        }
    }

    let mut files = BTreeMap::new();
    collect(root, root, &mut files);
    files
}

#[test]
fn inspect_verifies_the_graph_and_reports_only_root_local_files() {
    let repo = runtime_repo("runtime-inspect-local");
    let output = run(&repo, &["inspect", "decisions", "--json"]);
    assert!(
        output.status.success(),
        "inspect failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: Value = serde_json::from_slice(&output.stdout).expect("inspect JSON");
    assert_eq!(value["summary"]["total_files"], 1);
    assert_eq!(value["files"][0]["path"], "decisions/root.md");
    assert!(!String::from_utf8_lossy(&output.stdout).contains(PARENT_ID));
    assert!(!String::from_utf8_lossy(&output.stdout).contains("vendor/parent"));
}

#[test]
fn inspect_fails_closed_when_an_inherited_pin_is_tampered() {
    let repo = runtime_repo("runtime-inspect-tamper");
    repo.write_node(
        "vendor/parent",
        "decisions/parent.md",
        &decision(PARENT_ID, "Tampered Runtime Policy", None),
    );
    let output = run(&repo, &["inspect", "decisions", "--json"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("parent-corpus-digest-mismatch"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn inspect_refuses_direct_inherited_paths_before_a_parent_config_can_hide_the_graph() {
    let repo = runtime_repo("runtime-inspect-direct-parent");
    repo.write_node(
        "vendor/parent",
        "decisions/parent.md",
        &decision(PARENT_ID, "Tampered Direct Parent", None),
    );

    for target in [
        "vendor/parent/decisions",
        "vendor/parent/decisions/parent.md",
    ] {
        let output = run(&repo, &["inspect", target, "--json"]);
        assert_eq!(
            output.status.code(),
            Some(2),
            "direct inherited inspect must be refused: {target}\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(output.stdout.is_empty());
        assert!(
            String::from_utf8_lossy(&output.stderr)
                .contains("limited to root-local artifacts"),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[test]
fn named_write_surfaces_refuse_traversal_and_nonexistent_parent_suffixes_atomically() {
    let repo = runtime_repo("runtime-write-surfaces");
    fs::create_dir_all(repo.path("vendor/sibling")).expect("create traversal sibling");
    fs::create_dir_all(repo.path("vendor/parent/.git")).expect("create inherited git dir");
    repo.write("paths.txt", "src/guarded.rs\n");
    let parent = repo.path("vendor/parent");
    let before = file_bytes(&parent);

    assert_read_only_failure(
        &run(
            &repo,
            &[
                "herald",
                "decisions",
                "--paths-file",
                "paths.txt",
                "--out",
                "vendor/sibling/../parent/not-created/herald.md",
            ],
        ),
        "Herald traversal output",
    );
    assert_read_only_failure(
        &run(
            &repo,
            &[
                "herald",
                "decisions",
                "--paths-file",
                "paths.txt",
                "--out",
                "local-herald.md",
                "--github-output",
                "vendor/parent/not-created/github/output.txt",
            ],
        ),
        "Herald GitHub output",
    );
    assert!(!repo.path("local-herald.md").exists());

    assert_read_only_failure(
        &run(
            &repo,
            &["skill", "install", "--dir", "vendor/parent", "--json"],
        ),
        "skill installation",
    );
    assert_read_only_failure(
        &run(
            &repo,
            &["hook", "install", "--dir", "vendor/parent", "--json"],
        ),
        "hook installation",
    );
    assert_read_only_failure(
        &run(
            &repo,
            &[
                "eval",
                "--update-baseline",
                "--baseline",
                "vendor/parent/not-created/deep/baseline.json",
            ],
        ),
        "eval baseline update",
    );

    assert_eq!(file_bytes(&parent), before, "parent bytes changed");
}

#[cfg(unix)]
#[test]
fn concrete_multi_file_targets_refuse_symlink_routes_before_any_write() {
    use std::os::unix::fs::symlink;

    let repo = runtime_repo("runtime-write-symlinks");
    let parent = repo.path("vendor/parent");
    let before = file_bytes(&parent);

    fs::create_dir_all(repo.path("generated")).expect("create agent-rules output root");
    symlink(&parent, repo.path("generated/.github")).expect("link agent-rules subdirectory");
    assert_read_only_failure(
        &run(
            &repo,
            &[
                "export",
                "decisions",
                "--agent-rules",
                "--client",
                "copilot",
                "--out",
                "generated",
            ],
        ),
        "agent-rules symlink target",
    );

    fs::create_dir_all(repo.path("okf-out")).expect("create OKF output root");
    symlink(
        parent.join("decisions/parent.md"),
        repo.path("okf-out/index.md"),
    )
    .expect("link OKF generated file");
    assert_read_only_failure(
        &run(
            &repo,
            &["export", "decisions", "--okf", "--out", "okf-out"],
        ),
        "OKF bundle symlink target",
    );

    fs::create_dir_all(repo.path("local-skill/.claude")).expect("create skill output root");
    symlink(&parent, repo.path("local-skill/.claude/skills"))
        .expect("link skill destination tree");
    assert_read_only_failure(
        &run(
            &repo,
            &[
                "skill",
                "install",
                "decided-artifacts",
                "--dir",
                "local-skill",
            ],
        ),
        "skill symlink target",
    );

    assert_eq!(file_bytes(&parent), before, "parent bytes changed");
}
