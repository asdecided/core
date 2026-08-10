//! DecisionGrounding certification for the activated version-2 corpus graph.

use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../fixtures/eval")
}

fn run(args: &[String]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_decided"))
        .args(args)
        .output()
        .expect("run decided eval")
}

fn graph_args(mode: &str) -> Vec<String> {
    let root = fixtures().join("federation");
    vec![
        "eval".to_string(),
        mode.to_string(),
        "--root".to_string(),
        root.join("graph-decisions").to_string_lossy().into_owned(),
        "--queries".to_string(),
        root.join("graph-track/queries.json")
            .to_string_lossy()
            .into_owned(),
        "--baseline".to_string(),
        root.join("graph-track/baseline.json")
            .to_string_lossy()
            .into_owned(),
        "--config".to_string(),
        root.join("graph-track/eval-config.json")
            .to_string_lossy()
            .into_owned(),
    ]
}

#[test]
fn graph_track_gates_combined_ranking_floor_and_transitive_decisions() {
    let checked = run(&graph_args("--check"));
    assert!(
        checked.status.success(),
        "stdout={}, stderr={}",
        String::from_utf8_lossy(&checked.stdout),
        String::from_utf8_lossy(&checked.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&checked.stdout),
        "decided eval: gate PASS\n"
    );

    let first = run(&graph_args("--json"));
    let second = run(&graph_args("--json"));
    assert!(first.status.success());
    assert!(second.status.success());
    let first: Value = serde_json::from_slice(&first.stdout).expect("first scorecard JSON");
    let second: Value = serde_json::from_slice(&second.stdout).expect("second scorecard JSON");
    assert_eq!(first["metrics"], second["metrics"]);
    assert_eq!(first["per_query"], second["per_query"]);
    assert_eq!(first["metadata"]["n_queries"], 4);
    assert_eq!(first["metrics"]["overall"]["negative_violations"], 0);

    let queries = first["per_query"].as_array().expect("per-query rows");
    let gq01 = queries.iter().find(|row| row["id"] == "GQ01").unwrap();
    let large_parent = gq01["returned"].as_array().unwrap();
    assert_eq!(large_parent[0], "FEDEVAL-000000000001");
    assert!(
        large_parent
            .iter()
            .position(|id| id == "FEDEVAL-000000000002")
            .is_some_and(|position| position >= 5),
        "the high-inbound lexical hard negative entered the top-five window"
    );

    let gq03 = queries.iter().find(|row| row["id"] == "GQ03").unwrap();
    assert_eq!(gq03["returned"][0], "FEDEVAL-000000000004");
    let gq04 = queries.iter().find(|row| row["id"] == "GQ04").unwrap();
    assert_eq!(gq04["returned"][0], "FEDEVAL-000000000005");
}

#[test]
fn no_manifest_track_retains_its_committed_metrics() {
    let root = fixtures();
    let output = run(&[
        "eval".to_string(),
        "--json".to_string(),
        "--root".to_string(),
        root.join("corpus").to_string_lossy().into_owned(),
        "--queries".to_string(),
        root.join("queries.json").to_string_lossy().into_owned(),
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let scorecard: Value = serde_json::from_slice(&output.stdout).expect("scorecard JSON");
    let baseline: Value = serde_json::from_slice(
        &std::fs::read(root.join("baseline.json")).expect("read committed baseline"),
    )
    .expect("baseline JSON");
    assert_eq!(scorecard["metrics"], baseline);
}
