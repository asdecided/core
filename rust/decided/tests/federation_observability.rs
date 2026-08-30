//! Black-box contract for the manifest-v2 operator observability surface.

mod federation_graph_support;

use federation_graph_support::{decision, GraphRepo, OverrideEdge, ParentEdge, SHARED_ID};
use serde_json::Value;
use std::path::Path;
use std::process::{Command, Output};

const LEFT_SOURCE: &str = "acme/left";
const RIGHT_SOURCE: &str = "acme/right";
const REPLACEMENT_ID: &str = "APP-01K000000002";
const RATIONALE_ID: &str = "APP-01K000000003";
const DUPLICATE_ID: &str = "DUP-01K000000001";

fn observable_diamond(label: &str) -> GraphRepo {
    let repo = GraphRepo::new(label);
    repo.create_node("vendor/left", "LEFT", LEFT_SOURCE);
    repo.create_node("vendor/right", "RIGHT", RIGHT_SOURCE);
    repo.create_node("vendor/left/vendor/shared", "SHARED", "acme/shared");
    repo.write_node(
        "vendor/left/vendor/shared",
        "decisions/shared.md",
        &decision(SHARED_ID, "Shared Policy", None),
    );
    repo.copy_node("vendor/left/vendor/shared", "vendor/right/vendor/shared");

    let shared_pin = repo.v2_digest("vendor/left/vendor/shared", "acme/shared");
    assert_eq!(
        shared_pin,
        repo.v2_digest("vendor/right/vendor/shared", "acme/shared")
    );
    repo.write_v2_manifest(
        "vendor/left",
        &[ParentEdge::new(
            "shared",
            "acme/shared",
            "vendor/shared",
            shared_pin.clone(),
        )],
        &[],
    );
    repo.write_v2_manifest(
        "vendor/right",
        &[ParentEdge::new(
            "shared",
            "acme/shared",
            "vendor/shared",
            shared_pin,
        )],
        &[],
    );

    let left_pin = repo.v2_digest("vendor/left", LEFT_SOURCE);
    let right_pin = repo.v2_digest("vendor/right", RIGHT_SOURCE);
    repo.write_node(
        "",
        "decisions/replacement.md",
        &decision(REPLACEMENT_ID, "Application Policy", None),
    );
    repo.write_node(
        "",
        "decisions/rationale.md",
        &decision(RATIONALE_ID, "Application Override Rationale", None),
    );
    repo.write_v2_manifest(
        "",
        &[
            ParentEdge::new("left", LEFT_SOURCE, "vendor/left", left_pin),
            ParentEdge::new("right", RIGHT_SOURCE, "vendor/right", right_pin),
        ],
        &[OverrideEdge::new(
            &format!("acme/shared::{SHARED_ID}"),
            REPLACEMENT_ID,
            RATIONALE_ID,
        )],
    );
    repo
}

fn ambiguous_siblings(label: &str) -> GraphRepo {
    let repo = GraphRepo::new(label);
    repo.create_node("vendor/left", "LEFT", LEFT_SOURCE);
    repo.create_node("vendor/right", "RIGHT", RIGHT_SOURCE);
    repo.write_node(
        "vendor/left",
        "decisions/duplicate.md",
        &decision(DUPLICATE_ID, "Left Duplicate", None),
    );
    repo.write_node(
        "vendor/right",
        "decisions/duplicate.md",
        &decision(DUPLICATE_ID, "Right Duplicate", None),
    );
    let left_pin = repo.v2_digest("vendor/left", LEFT_SOURCE);
    let right_pin = repo.v2_digest("vendor/right", RIGHT_SOURCE);
    repo.write_v2_manifest(
        "",
        &[
            ParentEdge::new("left", LEFT_SOURCE, "vendor/left", left_pin),
            ParentEdge::new("right", RIGHT_SOURCE, "vendor/right", right_pin),
        ],
        &[],
    );
    repo
}

fn run(repo: &GraphRepo, args: &[&str]) -> Output {
    run_at(repo.root(), args)
}

fn run_at(root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_decided"))
        .args(args)
        .current_dir(root)
        .env("DECIDED_NO_CACHE", "1")
        .env("XDG_CACHE_HOME", root.join(".xdg/cache"))
        .env("XDG_CONFIG_HOME", root.join(".xdg/config"))
        .env("XDG_STATE_HOME", root.join(".xdg/state"))
        .output()
        .expect("run federation observability command")
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

#[test]
fn status_reports_the_verified_diamond_without_checkout_identity() {
    let repo = observable_diamond("observability-status");
    assert!(
        !repo.path(".git").exists(),
        "forge-neutral contract must not need Git metadata"
    );
    let output = run(&repo, &["corpus", "status", "decisions", "--json"]);
    assert!(
        output.status.success(),
        "status failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value = json(&output);
    assert_eq!(value["schema_version"], "1");
    assert_eq!(value["status"], "verified");
    assert_eq!(value["manifest_version"], 2);
    assert_eq!(value["verification"]["network_access"], false);
    assert_eq!(value["verification"]["pins_verified"], true);
    assert_eq!(value["summary"]["sources"], 4);
    assert_eq!(value["summary"]["inherited_sources"], 3);
    assert_eq!(value["summary"]["edges"], 4);
    assert_eq!(value["summary"]["physical_routes"], 4);
    assert_eq!(value["summary"]["max_depth"], 2);
    assert_eq!(value["summary"]["catalog_artifacts"], 4);
    assert_eq!(value["summary"]["effective_artifacts"], 3);
    assert_eq!(value["summary"]["root_local_artifacts"], 3);
    assert_eq!(value["summary"]["overrides"], 1);

    let shared = value["sources"]
        .as_array()
        .unwrap()
        .iter()
        .find(|source| source["source"] == "acme/shared")
        .expect("shared source row");
    assert_eq!(shared["route_count"], 2);
    assert_eq!(
        shared["source_route"],
        serde_json::json!(["acme/root", "acme/left", "acme/shared"])
    );
    assert_eq!(shared["writable"], false);
    assert!(value["edges"]
        .as_array()
        .unwrap()
        .iter()
        .all(|edge| edge["declared_pin_verified"] == true && edge["read_only"] == true));
    assert!(
        !String::from_utf8_lossy(&output.stdout)
            .contains(&repo.root().to_string_lossy().to_string()),
        "checkout path leaked into stable report"
    );

    let second_repo = observable_diamond("observability-status-copy");
    let second = run(&second_repo, &["corpus", "status", "decisions", "--json"]);
    assert!(second.status.success());
    assert_eq!(
        output.stdout, second.stdout,
        "stable status bytes changed with checkout location"
    );
}

#[test]
fn corpus_observability_defaults_to_the_conventional_decisions_directory() {
    let repo = observable_diamond("observability-default");
    let status = run(&repo, &["corpus", "status", "--json"]);
    assert!(
        status.status.success(),
        "default status failed: {}",
        String::from_utf8_lossy(&status.stderr)
    );
    assert_eq!(json(&status)["root_corpus"], "decisions");

    let reference = format!("acme/shared::{SHARED_ID}");
    let explain = run(&repo, &["corpus", "explain", &reference, "--json"]);
    assert!(
        explain.status.success(),
        "default explain failed: {}",
        String::from_utf8_lossy(&explain.stderr)
    );
    assert_eq!(json(&explain)["context"], "acme/root");
}

#[test]
fn explain_shows_history_terminal_and_explicit_override_provenance() {
    let repo = observable_diamond("observability-explain");
    let reference = format!("acme/shared::{SHARED_ID}");
    let output = run(
        &repo,
        &["corpus", "explain", &reference, "decisions", "--json"],
    );
    assert!(
        output.status.success(),
        "explain failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value = json(&output);
    assert_eq!(value["outcome"], "resolved");
    assert_eq!(value["qualified"], true);
    assert_eq!(
        value["selected"]["key"]["qualified_id"],
        format!("acme/shared::{SHARED_ID}")
    );
    assert_eq!(
        value["effective_terminal"]["key"]["qualified_id"],
        format!("acme/root::{REPLACEMENT_ID}")
    );
    assert_eq!(value["selected"]["provenance"]["layer"], "inherited");
    assert_eq!(value["effective_terminal"]["provenance"]["layer"], "local");
    assert_eq!(value["override_provenance"].as_array().unwrap().len(), 1);
    assert_eq!(value["override_provenance"][0]["state"], "overridden");
    assert_eq!(
        value["override_provenance"][0]["rationale"]["canonical_id"],
        RATIONALE_ID
    );
}

#[test]
fn explain_failure_is_machine_readable_and_nonzero() {
    let repo = observable_diamond("observability-not-found");
    let output = run(
        &repo,
        &["corpus", "explain", "DOES-NOT-EXIST", "decisions", "--json"],
    );
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stderr.is_empty());
    let value = json(&output);
    assert_eq!(value["outcome"], "not-found");
    assert_eq!(value["context"], "acme/root");
    assert!(value["historical_candidates"]
        .as_array()
        .unwrap()
        .is_empty());
}

#[test]
fn explain_emits_every_documented_diagnostic_outcome() {
    let repo = observable_diamond("observability-diagnostics");
    let cases = [
        (
            vec![
                "corpus",
                "explain",
                SHARED_ID,
                "decisions",
                "--from",
                "acme/unknown",
                "--json",
            ],
            "unknown-context",
        ),
        (
            vec![
                "corpus",
                "explain",
                "too::many::parts",
                "decisions",
                "--json",
            ],
            "invalid-reference",
        ),
        (
            vec![
                "corpus",
                "explain",
                "acme/shared::shared",
                "decisions",
                "--json",
            ],
            "canonical-id-required",
        ),
    ];
    for (args, expected) in cases {
        let output = run(&repo, &args);
        assert_eq!(output.status.code(), Some(1), "{expected}");
        assert!(output.stderr.is_empty(), "{expected}");
        assert_eq!(json(&output)["outcome"], expected);
    }

    let ambiguous = ambiguous_siblings("observability-ambiguous");
    let output = run(
        &ambiguous,
        &["corpus", "explain", DUPLICATE_ID, "decisions", "--json"],
    );
    assert_eq!(output.status.code(), Some(1));
    let value = json(&output);
    assert_eq!(value["outcome"], "ambiguous");
    assert_eq!(value["historical_candidates"].as_array().unwrap().len(), 2);
    assert_eq!(value["effective_candidates"].as_array().unwrap().len(), 2);
}

#[test]
fn explain_honors_source_local_alias_context() {
    let repo = observable_diamond("observability-context");
    let reference = format!("shared::{SHARED_ID}");
    let root = run(
        &repo,
        &["corpus", "explain", &reference, "decisions", "--json"],
    );
    assert_eq!(root.status.code(), Some(1));
    assert_eq!(json(&root)["outcome"], "not-found");

    let branch = run(
        &repo,
        &[
            "corpus",
            "explain",
            &reference,
            "decisions",
            "--from",
            LEFT_SOURCE,
            "--json",
        ],
    );
    assert!(
        branch.status.success(),
        "contextual explain failed: {}",
        String::from_utf8_lossy(&branch.stderr)
    );
    let value = json(&branch);
    assert_eq!(value["context"], LEFT_SOURCE);
    assert_eq!(
        value["selected"]["key"]["qualified_id"],
        format!("acme/shared::{SHARED_ID}")
    );
    assert_eq!(value["selected"], value["effective_terminal"]);
}

#[test]
fn status_fails_closed_before_rendering_when_any_route_is_tampered() {
    let repo = observable_diamond("observability-tamper");
    repo.write_node(
        "vendor/right/vendor/shared",
        "decisions/shared.md",
        &decision(SHARED_ID, "Tampered Shared Policy", None),
    );
    let output = run(&repo, &["corpus", "status", "decisions", "--json"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("parent-corpus-digest-mismatch"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn human_status_and_explain_keep_the_operator_contract_legible() {
    let repo = observable_diamond("observability-human");
    let status = run(&repo, &["corpus", "status", "decisions"]);
    assert!(status.status.success());
    let status_text = String::from_utf8_lossy(&status.stdout);
    assert!(status_text.contains("PASS  Federation verified"));
    assert!(status_text.contains("acme/shared [inherited]"));
    assert!(status_text.contains("2 physical routes"));
    assert!(status_text.contains("Read-only boundaries"));

    let reference = format!("acme/shared::{SHARED_ID}");
    let explain = run(&repo, &["corpus", "explain", &reference, "decisions"]);
    assert!(explain.status.success());
    let explain_text = String::from_utf8_lossy(&explain.stdout);
    assert!(explain_text.contains("RESOLVED"));
    assert!(explain_text.contains("Override provenance"));
    assert!(explain_text.contains(RATIONALE_ID));
}

#[test]
fn published_example_remains_fully_pinned_and_runnable() {
    let example = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("examples/federation/app");
    let status = run_at(&example, &["corpus", "status", "decisions", "--json"]);
    assert!(
        status.status.success(),
        "published example status failed: {}",
        String::from_utf8_lossy(&status.stderr)
    );
    let status_value = json(&status);
    assert_eq!(status_value["root_source"], "example/app");
    assert_eq!(status_value["summary"]["sources"], 4);
    assert_eq!(status_value["summary"]["physical_routes"], 4);

    let explain = run_at(
        &example,
        &[
            "corpus",
            "explain",
            "example/shared::SHR-01K000000001",
            "decisions",
            "--json",
        ],
    );
    assert!(
        explain.status.success(),
        "published example explain failed: {}",
        String::from_utf8_lossy(&explain.stderr)
    );
    assert_eq!(
        json(&explain)["effective_terminal"]["key"]["canonical_id"],
        "APP-01K000000001"
    );

    let validate = run_at(&example, &["validate", "decisions", "--no-cache"]);
    assert!(
        validate.status.success(),
        "published example validation failed: {}",
        String::from_utf8_lossy(&validate.stderr)
    );
}
