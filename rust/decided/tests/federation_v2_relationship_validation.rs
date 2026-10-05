//! Relationship validation in a manifest-v2 root (#504): the row-based
//! validator runs over the verified graph, so counts and every non-fatal
//! check apply exactly as they do in a single corpus. Error-severity
//! reference findings stay fatal when the graph is verified.

mod federation_graph_support;

use federation_graph_support::{
    decision, GraphRepo, ParentEdge, ROOT_ID, SHARED_ID, SHARED_SOURCE,
};
use serde_json::Value;
use std::process::{Command, Output};

const SECOND_ID: &str = "APP-01K000000002";

fn with_section(markdown: String, heading: &str, entry: &str) -> String {
    format!("{markdown}\n## {heading}\n\n- {entry}\n")
}

/// A root inheriting one parent, with the root decision citing the parent
/// by alias. `second` is written as the root's second decision.
fn federated_root(label: &str, second: &str) -> GraphRepo {
    let repo = GraphRepo::new(label);
    repo.create_node("vendor/shared", "SHARED", SHARED_SOURCE);
    repo.write_node(
        "vendor/shared",
        "decisions/shared.md",
        &decision(SHARED_ID, "Shared Policy", None),
    );
    let pin = repo.v2_digest("vendor/shared", SHARED_SOURCE);
    repo.write_v2_manifest(
        "",
        &[ParentEdge::new(
            "shared",
            SHARED_SOURCE,
            "vendor/shared",
            pin,
        )],
        &[],
    );
    repo.write(
        "decisions/root.md",
        &decision(
            ROOT_ID,
            "Root Policy",
            Some(&format!("shared::{SHARED_ID}")),
        ),
    );
    repo.write("decisions/second.md", second);
    repo
}

fn run(repo: &GraphRepo, args: &[&str]) -> Output {
    let root = repo.root();
    Command::new(env!("CARGO_BIN_EXE_decided"))
        .args(args)
        .current_dir(root)
        .env("DECIDED_NO_CACHE", "1")
        .env("XDG_CACHE_HOME", root.join(".xdg/cache"))
        .env("XDG_CONFIG_HOME", root.join(".xdg/config"))
        .env("XDG_STATE_HOME", root.join(".xdg/state"))
        .output()
        .expect("run decided")
}

fn json(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "invalid JSON: {error}\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

fn issue_codes(report: &Value) -> Vec<String> {
    report["issues"]
        .as_array()
        .expect("issues array")
        .iter()
        .map(|issue| issue["code"].as_str().expect("code").to_string())
        .collect()
}

#[test]
fn federated_root_counts_cross_source_and_local_references() {
    let repo = federated_root(
        "relationship-count",
        &decision(SECOND_ID, "Second Policy", Some(ROOT_ID)),
    );

    let output = run(
        &repo,
        &["relationships", "decisions", "--validate", "--json"],
    );
    let report = json(&output);
    assert_eq!(output.status.code(), Some(0), "{report}");
    assert_eq!(report["relationships_checked"], 2, "{report}");
    assert!(issue_codes(&report).is_empty(), "{report}");

    let human = run(&repo, &["relationships", "decisions", "--validate"]);
    let stdout = String::from_utf8_lossy(&human.stdout);
    assert!(stdout.contains("Relationships Checked: 2"), "{stdout}");
}

#[test]
fn federated_root_reports_a_missing_applies_to_path_and_gate_blocks() {
    let repo = federated_root(
        "relationship-scope",
        &with_section(
            decision(SECOND_ID, "Second Policy", None),
            "Applies To",
            "missing/dir/",
        ),
    );

    let output = run(
        &repo,
        &["relationships", "decisions", "--validate", "--json"],
    );
    let report = json(&output);
    assert_eq!(output.status.code(), Some(1), "{report}");
    assert_eq!(
        issue_codes(&report),
        ["applies-to-target-not-found"],
        "{report}"
    );

    let gate = run(&repo, &["gate", "decisions", "--json"]);
    let verdict = json(&gate);
    assert_eq!(gate.status.code(), Some(1), "{verdict}");
    assert_eq!(verdict["ok"], false, "{verdict}");
}

#[test]
fn federated_root_warns_when_a_root_decision_cites_a_superseded_parent() {
    let repo = federated_root(
        "relationship-superseded",
        &decision(SECOND_ID, "Second Policy", None),
    );
    repo.write_node(
        "vendor/shared",
        "decisions/shared.md",
        &decision(SHARED_ID, "Shared Policy", None)
            .replace("## Status\n\nAccepted", "## Status\n\nSuperseded"),
    );
    let pin = repo.v2_digest("vendor/shared", SHARED_SOURCE);
    repo.write_v2_manifest(
        "",
        &[ParentEdge::new(
            "shared",
            SHARED_SOURCE,
            "vendor/shared",
            pin,
        )],
        &[],
    );

    let output = run(
        &repo,
        &["relationships", "decisions", "--validate", "--json"],
    );
    let report = json(&output);
    assert_eq!(
        issue_codes(&report),
        ["relationship-target-superseded"],
        "{report}"
    );
}
