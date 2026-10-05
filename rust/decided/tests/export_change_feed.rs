//! Black-box contract for `decided export --documents|--graph --since <rev>`
//! (#258): change classification on canonical id, resolved cursors, the
//! empty feed, and the replay law against real Git history.

use rac_engine::export_feed::{apply_documents_feed, apply_graph_feed, WORKING_TREE};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static SCRATCH_SEQUENCE: AtomicU64 = AtomicU64::new(0);

struct TestRepo {
    root: PathBuf,
    runtime: PathBuf,
}

impl TestRepo {
    fn new(label: &str) -> Self {
        let sequence = SCRATCH_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock after epoch")
            .as_nanos();
        let suffix = format!("{}-{nonce}-{sequence}", std::process::id());
        let root = std::env::temp_dir().join(format!("asdecided-feed-{label}-{suffix}"));
        let runtime = std::env::temp_dir().join(format!("asdecided-feed-{label}-runtime-{suffix}"));
        fs::create_dir_all(&root).expect("create feed repository");
        fs::create_dir_all(&runtime).expect("create feed runtime directory");
        let repo = Self { root, runtime };
        repo.git(&["init", "--quiet"]);
        repo.git(&["config", "user.name", "AsDecided Test"]);
        repo.git(&["config", "user.email", "test@asdecided.invalid"]);
        repo.git(&["config", "core.autocrlf", "false"]);
        repo.write(
            ".decided/config.yaml",
            "repository_key: FEED\ncorpus:\n  source: tests/change-feed\n",
        );
        repo
    }

    fn write(&self, relative: &str, contents: &str) {
        let path = self.root.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create fixture parent directory");
        }
        fs::write(path, contents).expect("write feed fixture");
    }

    fn git(&self, args: &[&str]) -> String {
        let output = Command::new("git")
            .args(args)
            .current_dir(&self.root)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .expect("run git for feed fixture");
        assert!(
            output.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }

    fn commit_all(&self, message: &str) -> String {
        self.git(&["add", "--all"]);
        self.git(&["commit", "--quiet", "--message", message]);
        self.git(&["rev-parse", "HEAD"])
    }

    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_decided"))
            .args(args)
            .current_dir(&self.root)
            .env("DECIDED_CACHE_DIR", self.runtime.join("decided-cache"))
            .env("XDG_CACHE_HOME", self.runtime.join("xdg-cache"))
            .env("XDG_CONFIG_HOME", self.runtime.join("xdg-config"))
            .env("XDG_STATE_HOME", self.runtime.join("xdg-state"))
            .output()
            .expect("run decided export")
    }

    fn stdout(&self, args: &[&str]) -> String {
        let output = self.run(args);
        assert!(
            output.status.success(),
            "decided {args:?} failed with {:?}\nstdout:\n{}\nstderr:\n{}",
            output.status.code(),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).expect("UTF-8 stdout")
    }
}

impl Drop for TestRepo {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
        let _ = fs::remove_dir_all(&self.runtime);
    }
}

fn decision(id: &str, title: &str, marker: &str, related: Option<&str>) -> String {
    let mut body = format!(
        "---\nschema_version: 1\nid: {id}\ntype: decision\n---\n# {title}\n\n## Status\n\nAccepted\n\n## Context\n\nFeeds must be incremental.\n\n## Decision\n\n{marker}\n\n## Consequences\n\nConsumers re-embed only what changed.\n"
    );
    if let Some(target) = related {
        body.push_str(&format!("\n## Related Decisions\n\n- {target}\n"));
    }
    body
}

const A: &str = "FEED-01K00000000A";
const B: &str = "FEED-01K00000000B";
const C: &str = "FEED-01K00000000C";
const D: &str = "FEED-01K00000000D";

/// Commit one: decisions A, B, C. Returns the repository and the commit.
fn three_decisions(label: &str) -> (TestRepo, String) {
    let repo = TestRepo::new(label);
    repo.write("decisions/a.md", &decision(A, "Alpha", "one", None));
    repo.write("decisions/b.md", &decision(B, "Beta", "two", Some(A)));
    repo.write("decisions/c.md", &decision(C, "Gamma", "three", None));
    let commit = repo.commit_all("base");
    (repo, commit)
}

fn lines(text: &str) -> Vec<Value> {
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).expect("feed line is JSON"))
        .collect()
}

#[test]
fn one_change_of_each_kind_is_classified_cursored_stable_and_replayable() {
    let (repo, base) = three_decisions("kinds");
    repo.write(
        "decisions/b.md",
        &decision(B, "Beta", "two, revised", Some(A)),
    );
    fs::remove_file(repo.root.join("decisions/c.md")).expect("delete C");
    repo.write("decisions/d.md", &decision(D, "Delta", "four", None));
    let head = repo.commit_all("head");

    let args = [
        "export",
        "decisions",
        "--documents",
        "--since",
        &base,
        "--at",
        &head,
    ];
    let feed = repo.stdout(&args);
    assert_eq!(
        feed,
        repo.stdout(&args),
        "feed bytes are stable across runs"
    );

    let records = lines(&feed);
    assert_eq!(records[0]["feed"], "documents");
    assert_eq!(records[0]["base"], base.as_str());
    assert_eq!(records[0]["head"], head.as_str());
    let changes: Vec<(&str, &str)> = records[1..]
        .iter()
        .map(|record| {
            let change = record["change"].as_str().unwrap();
            let id = if change == "removed" {
                record["id"].as_str().unwrap()
            } else {
                record["document"]["id"].as_str().unwrap()
            };
            (change, id)
        })
        .collect();
    assert_eq!(
        changes,
        [("modified", B), ("removed", C), ("added", D)],
        "{feed}"
    );
    assert_eq!(records[2]["path"], "decisions/c.md");

    let base_export = repo.stdout(&["export", "decisions", "--documents", "--at", &base]);
    let head_export = repo.stdout(&["export", "decisions", "--documents", "--at", &head]);
    // Embedded records are the head export's own lines.
    let head_lines: BTreeSet<&str> = head_export.lines().collect();
    for record in records[1..]
        .iter()
        .filter(|record| record["change"] != "removed")
    {
        let embedded = rac_engine::pyjson::dumps_compact(&record["document"]);
        assert!(head_lines.contains(embedded.as_str()), "{embedded}");
    }
    assert_eq!(
        apply_documents_feed(&base_export, &feed).expect("replay"),
        head_export.trim_end_matches('\n'),
        "base + feed reproduces the head export byte-for-byte"
    );
}

#[test]
fn a_move_that_keeps_the_id_is_one_modified_record_with_the_new_path() {
    let (repo, base) = three_decisions("move");
    fs::create_dir_all(repo.root.join("decisions/archive")).expect("create archive");
    repo.git(&["mv", "decisions/c.md", "decisions/archive/c.md"]);
    let head = repo.commit_all("move");

    let feed = repo.stdout(&[
        "export",
        "decisions",
        "--documents",
        "--since",
        &base,
        "--at",
        &head,
    ]);
    let records = lines(&feed);
    assert_eq!(records.len(), 2, "{feed}");
    assert_eq!(records[1]["change"], "modified");
    assert_eq!(records[1]["document"]["id"], C);
    assert_eq!(
        records[1]["document"]["metadata"]["path"],
        "decisions/archive/c.md"
    );
}

#[test]
fn since_head_on_a_clean_tree_is_an_empty_feed() {
    let (repo, base) = three_decisions("empty");
    let feed = repo.stdout(&["export", "decisions", "--documents", "--since", "HEAD"]);
    let records = lines(&feed);
    assert_eq!(records.len(), 1, "only the cursor line: {feed}");
    assert_eq!(records[0]["base"], base.as_str());
    assert_eq!(records[0]["head"], WORKING_TREE);

    let graph: Value =
        serde_json::from_str(&repo.stdout(&["export", "decisions", "--graph", "--since", "HEAD"]))
            .expect("graph feed is JSON");
    for field in [
        "nodes_added",
        "nodes_modified",
        "nodes_removed",
        "edges_added",
        "edges_removed",
    ] {
        assert_eq!(graph[field], Value::Array(Vec::new()), "{field}");
    }
}

#[test]
fn a_working_tree_head_sees_uncommitted_changes() {
    let (repo, base) = three_decisions("worktree");
    repo.write("decisions/d.md", &decision(D, "Delta", "four", None));
    let records = lines(&repo.stdout(&["export", "decisions", "--documents", "--since", &base]));
    assert_eq!(records[0]["head"], WORKING_TREE);
    assert_eq!(records.len(), 2);
    assert_eq!(records[1]["change"], "added");
}

#[test]
fn graph_feed_replays_the_head_node_and_edge_sets() {
    let (repo, base) = three_decisions("graph");
    repo.write("decisions/b.md", &decision(B, "Beta", "two", None));
    fs::remove_file(repo.root.join("decisions/c.md")).expect("delete C");
    repo.write("decisions/d.md", &decision(D, "Delta", "four", Some(A)));
    let head = repo.commit_all("graph head");

    let feed = repo.stdout(&[
        "export",
        "decisions",
        "--graph",
        "--since",
        &base,
        "--at",
        &head,
    ]);
    let parsed: Value = serde_json::from_str(&feed).expect("graph feed");
    assert_eq!(parsed["feed"], "graph");
    assert_eq!(parsed["nodes_added"][0]["id"], D);
    assert_eq!(parsed["nodes_removed"][0]["id"], C);
    assert_eq!(parsed["edges_added"].as_array().unwrap().len(), 1);
    assert_eq!(parsed["edges_removed"].as_array().unwrap().len(), 1);

    let base_graph = repo.stdout(&["export", "decisions", "--graph", "--at", &base]);
    let head_graph = repo.stdout(&["export", "decisions", "--graph", "--at", &head]);
    let replayed = apply_graph_feed(&base_graph, &feed).expect("replay");
    let (source, nodes, edges) = (replayed.source, replayed.nodes, replayed.edges);
    let head_value: Value = serde_json::from_str(&head_graph).expect("head graph");
    assert_eq!(source, head_value["source"]);
    let expected_nodes: BTreeMap<(String, String), String> = head_value["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|node| {
            (
                (
                    head_value["source"].as_str().unwrap().to_string(),
                    node["id"].as_str().unwrap().to_string(),
                ),
                rac_engine::pyjson::dumps_compact(node),
            )
        })
        .collect();
    assert_eq!(nodes, expected_nodes);
    let expected_edges: BTreeSet<String> = head_value["edges"]
        .as_array()
        .unwrap()
        .iter()
        .map(rac_engine::pyjson::dumps_compact)
        .collect();
    assert_eq!(edges, expected_edges);
}

#[test]
fn since_is_rejected_outside_documents_and_graph_and_needs_a_known_revision() {
    let (repo, _) = three_decisions("usage");
    for args in [
        vec!["export", "decisions", "--since", "HEAD"],
        vec!["export", "decisions", "--okf", "--since", "HEAD"],
        vec!["export", "--schema", "documents", "--since", "HEAD"],
        vec![
            "export",
            "decisions",
            "--documents",
            "--since",
            "no-such-revision",
        ],
        vec!["export", "decisions", "--documents", "--since="],
    ] {
        let output = repo.run(&args);
        assert_eq!(output.status.code(), Some(2), "{args:?}");
        assert!(output.stdout.is_empty(), "{args:?}");
    }
}
