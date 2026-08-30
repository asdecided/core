//! Cross-platform smoke tests for the native `decided` argv surface.

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static SCRATCH_SEQUENCE: AtomicU64 = AtomicU64::new(0);

fn scratch_suffix() -> String {
    let sequence = SCRATCH_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock after epoch")
        .as_nanos();
    format!("{}-{nonce}-{sequence}", std::process::id())
}

fn scratch_root() -> PathBuf {
    let root = std::env::temp_dir().join(format!("asdecided-cli-positional-{}", scratch_suffix()));
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
fn corpus_digest_version_two_is_explicit_and_keeps_v1_as_the_default() {
    let root = empty_scratch_root("digest-v2");
    fs::create_dir_all(root.join(".decided")).unwrap();
    fs::create_dir_all(root.join("decisions")).unwrap();
    fs::write(
        root.join(".decided/config.yaml"),
        "repository_key: STD\ncorpus:\n  source: acme/standards\n",
    )
    .unwrap();
    fs::write(root.join("decisions/policy.md"), "policy\n").unwrap();
    let root_text = root.to_string_lossy().into_owned();

    let v1 = run(&[
        "corpus",
        "digest",
        "--root",
        &root_text,
        "--corpus",
        "decisions",
    ]);
    assert!(v1.status.success(), "{}", String::from_utf8_lossy(&v1.stderr));
    assert!(String::from_utf8_lossy(&v1.stdout).starts_with("sha256:"));

    let v2 = run(&[
        "corpus",
        "digest",
        "--version",
        "2",
        "--root",
        &root_text,
        "--corpus",
        "decisions",
    ]);
    assert!(v2.status.success(), "{}", String::from_utf8_lossy(&v2.stderr));
    assert!(String::from_utf8_lossy(&v2.stdout).starts_with("sha256-v2:"));

    fs::remove_dir_all(root).unwrap();
}

fn empty_scratch_root(label: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("asdecided-cli-{label}-{}", scratch_suffix()));
    fs::create_dir_all(&root).expect("create empty CLI scratch repository");
    root
}

#[test]
fn init_parent_corpus_emits_guidance_without_creating_a_manifest() {
    let root = empty_scratch_root("parent-guidance");
    let root_text = root.to_string_lossy().into_owned();
    let output = run(&["init", &root_text, "--parent-corpus"]);

    assert!(
        output.status.success(),
        "stdout={}, stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    for expected in [
        "Parent corpus setup:",
        "Materialise the parent inside this repository",
        "decided corpus digest --root <parent-root> --corpus <parent-corpus>",
        ".decided/corpus.md",
        "## inherits",
        "## overrides",
    ] {
        assert!(stdout.contains(expected), "missing {expected:?}: {stdout}");
    }
    assert!(root.join(".decided/config.yaml").is_file());
    assert!(!root.join(".decided/corpus.md").exists());
    assert_eq!(
        fs::read_dir(root.join(".decided"))
            .expect("read .decided")
            .count(),
        1,
        "the guidance flag must not create another .decided file"
    );

    fs::remove_dir_all(root).expect("remove parent-guidance scratch repository");
}

#[test]
fn init_parent_corpus_is_profile_composable_and_idempotent() {
    let root = empty_scratch_root("parent-profile");
    let root_text = root.to_string_lossy().into_owned();
    let fresh = run(&[
        "init",
        &root_text,
        "--profile",
        "default",
        "--parent-corpus",
        "--json",
    ]);
    assert!(
        fresh.status.success(),
        "stdout={}, stderr={}",
        String::from_utf8_lossy(&fresh.stdout),
        String::from_utf8_lossy(&fresh.stderr)
    );
    let fresh_stdout = String::from_utf8_lossy(&fresh.stdout);
    assert!(fresh_stdout.contains("\"profile\": \"default\""));
    assert!(fresh_stdout.contains("\"parent_corpus_guidance\": {"));
    assert!(root.join(".mcp.json").is_file());
    assert!(root.join(".cursor/mcp.json").is_file());
    assert!(!root.join(".decided/corpus.md").exists());

    let before = fs::read(root.join(".decided/config.yaml")).expect("read config before re-init");
    let idempotent = run(&["init", &root_text, "--parent-corpus"]);
    assert!(
        idempotent.status.success(),
        "stdout={}, stderr={}",
        String::from_utf8_lossy(&idempotent.stdout),
        String::from_utf8_lossy(&idempotent.stderr)
    );
    let idempotent_stdout = String::from_utf8_lossy(&idempotent.stdout);
    assert!(idempotent_stdout.starts_with("Already initialized: repository key RAC\n"));
    assert!(idempotent_stdout.contains("Parent corpus setup:"));
    assert_eq!(
        fs::read(root.join(".decided/config.yaml")).expect("read config after re-init"),
        before
    );
    assert!(!root.join(".decided/corpus.md").exists());

    fs::remove_dir_all(root).expect("remove parent-profile scratch repository");
}

#[test]
fn init_parent_corpus_preserves_an_existing_non_default_key() {
    let root = empty_scratch_root("parent-existing-key");
    fs::create_dir_all(root.join(".decided")).expect("create existing config directory");
    let config = b"repository_key: APP\ncorpus:\n  source: acme/app\n";
    fs::write(root.join(".decided/config.yaml"), config).expect("write existing config");
    let root_text = root.to_string_lossy().into_owned();

    let guidance = run(&["init", &root_text, "--parent-corpus", "--json"]);
    assert!(
        guidance.status.success(),
        "stdout={}, stderr={}",
        String::from_utf8_lossy(&guidance.stdout),
        String::from_utf8_lossy(&guidance.stderr)
    );
    let guidance_stdout = String::from_utf8_lossy(&guidance.stdout);
    assert!(guidance_stdout.contains("\"repository_key\": \"APP\""));
    assert!(guidance_stdout.contains("\"created\": false"));
    assert!(guidance_stdout.contains("\"parent_corpus_guidance\": {"));
    assert_eq!(
        fs::read(root.join(".decided/config.yaml")).expect("read preserved config"),
        config
    );
    assert!(!root.join(".decided/corpus.md").exists());

    let explicit_conflict = run(&[
        "init",
        &root_text,
        "--parent-corpus",
        "--key",
        "RAC",
        "--json",
    ]);
    assert_eq!(explicit_conflict.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&explicit_conflict.stderr).contains(
        "repository already initialized with key 'APP'"
    ));

    fs::remove_dir_all(root).expect("remove existing-key scratch repository");
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
fn export_local_only_is_additive_for_the_three_read_projections() {
    let root = scratch_root();
    let root_text = root.to_string_lossy().into_owned();
    for mode in [None, Some("--documents"), Some("--graph")] {
        let mut baseline_args = vec!["export", &root_text];
        if let Some(mode) = mode {
            baseline_args.push(mode);
        }
        let baseline = run(&baseline_args);
        assert!(baseline.status.success());

        let mut local_args = baseline_args;
        local_args.push("--local-only");
        let local = run(&local_args);
        assert!(
            local.status.success(),
            "{mode:?} local projection failed: {}",
            String::from_utf8_lossy(&local.stderr)
        );
        assert_eq!(local.stdout, baseline.stdout);
        assert_eq!(local.stderr, baseline.stderr);
    }
    fs::remove_dir_all(root).expect("remove local-only export corpus");
}

#[test]
fn export_local_only_rejects_non_composed_modes() {
    let root = scratch_root();
    let root_text = root.to_string_lossy().into_owned();
    for args in [
        vec!["export", &root_text, "--okf", "--local-only"],
        vec!["export", &root_text, "--agent-rules", "--local-only"],
        vec!["export", "--schema", "viewer", "--local-only"],
    ] {
        let output = run(&args);
        assert_eq!(output.status.code(), Some(2));
        assert!(
            String::from_utf8_lossy(&output.stderr)
                .contains("--local-only is available only for viewer, documents, and graph exports"),
            "stderr={}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    fs::remove_dir_all(root).expect("remove local-only refusal corpus");
}

#[test]
fn export_rejects_an_invalid_configured_corpus_source() {
    let root = scratch_root();
    fs::create_dir_all(root.join(".decided")).unwrap();
    fs::write(
        root.join(".decided/config.yaml"),
        "repository_key: APP\ncorpus:\n  source: Not Namespaced\n",
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

#[test]
fn corpus_digest_prints_the_canonical_read_only_pin() {
    let root = scratch_root();
    fs::create_dir_all(root.join(".decided")).unwrap();
    fs::create_dir_all(root.join("decisions/sub")).unwrap();
    fs::write(
        root.join(".decided/config.yaml"),
        b"repository_key: STD\ncorpus:\n  source: acme/standards\n",
    )
    .unwrap();
    fs::write(root.join("decisions/a.md"), b"alpha\n").unwrap();
    fs::write(root.join("decisions/sub/b.md"), b"beta\r\n").unwrap();
    fs::write(root.join("decisions/ignored.MD"), b"ignored\n").unwrap();
    let before_config = fs::read(root.join(".decided/config.yaml")).unwrap();
    let before_a = fs::read(root.join("decisions/a.md")).unwrap();
    let root_text = root.to_string_lossy().into_owned();

    let output = run(&[
        "corpus",
        "digest",
        "--root",
        &root_text,
        "--corpus",
        "decisions",
    ]);
    assert!(
        output.status.success(),
        "stdout={}, stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        output.stdout,
        b"sha256:899d5cdfa52b90a157b018dceb20f4f2901e0d56c91b089c12286c0b8b7b3325\n"
    );
    assert!(output.stderr.is_empty());
    assert_eq!(fs::read(root.join(".decided/config.yaml")).unwrap(), before_config);
    assert_eq!(fs::read(root.join("decisions/a.md")).unwrap(), before_a);

    fs::remove_dir_all(root).expect("remove CLI digest corpus");
}

#[test]
fn corpus_digest_bounds_config_and_rejects_escaping_corpus_paths() {
    let root = scratch_root();
    fs::create_dir_all(root.join("parent/decisions")).unwrap();
    fs::write(root.join("parent/decisions/a.md"), b"alpha\n").unwrap();
    fs::create_dir_all(root.join(".decided")).unwrap();
    fs::write(
        root.join(".decided/config.yaml"),
        b"repository_key: CHILD\ncorpus:\n  source: acme/child\n",
    )
    .unwrap();
    let parent = root.join("parent").to_string_lossy().into_owned();

    let missing = run(&[
        "corpus",
        "digest",
        "--root",
        &parent,
        "--corpus",
        "decisions",
    ]);
    assert_eq!(missing.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&missing.stderr).contains("parent-corpus-config-missing")
    );

    let escaping = run(&[
        "corpus",
        "digest",
        "--root",
        &parent,
        "--corpus",
        "../decisions",
    ]);
    assert_eq!(escaping.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&escaping.stderr).contains("parent-corpus-path-escape")
    );

    fs::remove_dir_all(root).expect("remove CLI digest corpus");
}
