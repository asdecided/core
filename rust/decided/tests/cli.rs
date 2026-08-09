//! Cross-platform smoke tests for the native `decided` argv surface.

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

fn scratch_root() -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock after epoch")
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "asdecided-cli-positional-{}-{nonce}",
        std::process::id()
    ));
    fs::create_dir_all(&root).expect("create CLI smoke corpus");
    fs::write(
        root.join("decision.md"),
        "---\nschema_version: 1\nid: RAC-111111111111\ntype: decision\n---\n# Decision\n\n## Context\n\nA CLI parser smoke fixture.\n\n## Decision\n\nKeep the positional contract explicit.\n\n## Consequences\n\nThe native parser is tested on every supported platform.\n\n## Status\n\nAccepted\n",
    )
    .expect("write CLI smoke fixture");
    root
}

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_decided"))
        .args(args)
        .output()
        .expect("run decided")
}

#[test]
fn find_accepts_a_literal_hyphen_query() {
    let root = scratch_root();
    let root = root.to_string_lossy().into_owned();
    let output = run(&["find", "-", "--no-cache", &root]);

    assert!(
        output.status.success(),
        "find - should be a normal query, stdout={:?}, stderr={:?}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        !String::from_utf8_lossy(&output.stderr).contains("required: query"),
        "the literal hyphen was parsed as an option"
    );

    fs::remove_dir_all(root).expect("remove CLI smoke corpus");
}

#[test]
fn resolve_accepts_a_literal_hyphen_id_and_reaches_resolution() {
    let root = scratch_root();
    let root = root.to_string_lossy().into_owned();
    let output = run(&["resolve", "-", &root]);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert_eq!(
        output.status.code(),
        Some(1),
        "resolve - should be a normal not-found result"
    );
    assert!(
        stderr.contains("artifact not found: -"),
        "stderr={stderr:?}"
    );
    assert!(
        !stderr.contains("required: id"),
        "the literal hyphen was parsed as an option"
    );

    fs::remove_dir_all(root).expect("remove CLI smoke corpus");
}

#[test]
fn diagnose_requires_a_named_target() {
    let output = run(&["diagnose", "storage"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("the following arguments are required: target")
    );
}

#[test]
fn diagnose_emits_a_named_target_trace() {
    let root = scratch_root();
    let root_text = root.to_string_lossy().into_owned();
    let output = run(&[
        "diagnose",
        "explicit contract",
        "RAC-111111111111",
        &root_text,
        "--json",
    ]);
    assert!(
        output.status.success(),
        "stdout={}, stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("\"target\": \"RAC-111111111111\""));
    assert!(stdout.contains("\"outcome\": \"diagnosed\""));
    assert!(stdout.contains("\"reason\": \"surfaced\""));
    assert!(stdout.contains("\"rank\": 1"));

    fs::remove_dir_all(root).expect("remove CLI smoke corpus");
}

#[test]
fn generated_agent_rules_are_in_sync_with_the_decision_corpus() {
    let corpus = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../decisions");
    let corpus = corpus.to_string_lossy().into_owned();
    let output = run(&["export", &corpus, "--agent-rules", "--check"]);
    assert!(
        output.status.success(),
        "agent-rules drifted: stdout={}, stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn export_schema_prints_the_packaged_resource_exactly() {
    let output = run(&["export", "--schema", "documents"]);
    assert!(
        output.status.success(),
        "stdout={}, stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        output.stdout,
        include_bytes!("../../rac-engine/assets/schemas/export-documents-v1.schema.json")
    );
}

#[test]
fn export_schema_rejects_an_unknown_projection() {
    let output = run(&["export", "--schema", "unknown"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("invalid choice: 'unknown'"),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn export_rejects_an_invalid_configured_corpus_source() {
    let root = scratch_root();
    fs::create_dir_all(root.join(".decided")).unwrap();
    fs::write(
        root.join(".decided/config.yaml"),
        "repository_key: RAC\ncorpus:\n  source: Not Namespaced\n",
    )
    .unwrap();
    let root_text = root.to_string_lossy().into_owned();
    let output = run(&["export", &root_text, "--graph"]);

    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("invalid corpus.source"), "stderr={stderr}");
    assert!(
        stderr.contains("lower-case slash-namespaced"),
        "stderr={stderr}"
    );

    fs::remove_dir_all(root).expect("remove CLI smoke corpus");
}
