//! Pinned spec bundles (ADR-083, revised): the `rust/fixtures/spec-bundle/`
//! corpus declares `runbook` and `policy` plus a deliberately colliding
//! `decision` element. These tests pin the merged registry's behaviour end to
//! end through the CLI, and the golden guard: a corpus with no bundle stanza
//! sees exactly the built-in five.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../fixtures/spec-bundle")
        .canonicalize()
        .expect("spec-bundle fixture exists")
}

fn scratch(tag: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "asdecided-spec-bundle-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&root).unwrap();
    root
}

fn copy_dir(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).unwrap();
        }
    }
}

/// A private copy of the fixture, for tests that mutate it.
fn fixture_copy(tag: &str) -> PathBuf {
    let root = scratch(tag);
    copy_dir(&fixture(), &root);
    root
}

fn run_in(cwd: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_decided"))
        .args(args)
        .current_dir(cwd)
        .env("DECIDED_NO_CACHE", "1")
        .env("LC_ALL", "C")
        .env("TZ", "UTC")
        .output()
        .expect("run decided")
}

fn stdout(output: &Output) -> String {
    String::from_utf8(output.stdout.clone()).expect("UTF-8 stdout")
}

fn stderr(output: &Output) -> String {
    String::from_utf8(output.stderr.clone()).expect("UTF-8 stderr")
}

fn json(output: &Output) -> serde_json::Value {
    serde_json::from_str(&stdout(output)).unwrap_or_else(|e| {
        panic!(
            "stdout is not JSON: {e}\n{}\n{}",
            stdout(output),
            stderr(output)
        )
    })
}

#[test]
fn validate_admits_bundle_types_and_warns_on_the_builtin_collision() {
    let root = fixture();
    let output = run_in(&root, &["validate", "decisions", "--json"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let payload = json(&output);
    assert_eq!(payload["valid"], true);
    let bundle = &payload["artifact_spec_bundle"];
    assert_eq!(bundle["path"], ".decided/artifact-specs.json");
    assert_eq!(bundle["admitted"], serde_json::json!(["runbook", "policy"]));
    let warnings = bundle["warnings"].as_array().unwrap();
    assert_eq!(warnings.len(), 1);
    assert_eq!(warnings[0]["code"], "artifact-spec-skipped");
    assert_eq!(warnings[0]["name"], "decision");
    assert_eq!(warnings[0]["index"], 2);
    assert_eq!(warnings[0]["severity"], "warning");
    let files: Vec<(String, String, String)> = payload["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| {
            (
                f["path"].as_str().unwrap().to_string(),
                f["artifact_type"].as_str().unwrap().to_string(),
                f["status"].as_str().unwrap().to_string(),
            )
        })
        .collect();
    assert!(files.contains(&(
        "decisions/runbooks/deploy-search-service.md".into(),
        "runbook".into(),
        "valid".into()
    )));
    assert!(files.contains(&(
        "decisions/policies/data-retention.md".into(),
        "policy".into(),
        "valid".into()
    )));
    // The built-in decision is untouched by the shadowing element.
    assert!(files.contains(&(
        "decisions/decisions/adr-001-example.md".into(),
        "decision".into(),
        "valid".into()
    )));
    assert_eq!(payload["okf"]["conformant"], true);

    let human = run_in(&root, &["validate", "decisions"]);
    assert!(human.status.success());
    let text = stdout(&human);
    assert!(text.contains("WARN  .decided/artifact-specs.json  (artifact spec bundle)"));
    assert!(text.contains("[artifact-spec-skipped] element decision"));
    assert!(text.contains("PASS  decisions — 3 artifact(s) checked: 3 valid, 0 invalid."));
}

#[test]
fn registry_order_is_builtins_then_bundle_in_file_order() {
    let root = fixture();
    let output = run_in(&root, &["schema", "--list", "--json"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let payload = json(&output);
    let names: Vec<&str> = payload["schemas"]
        .as_array()
        .or_else(|| payload.as_array())
        .expect("schema list")
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert_eq!(
        names,
        [
            "requirement",
            "decision",
            "roadmap",
            "prompt",
            "design",
            "runbook",
            "policy"
        ]
    );

    let schema = run_in(&root, &["schema", "runbook", "--json"]);
    assert!(schema.status.success());
    let runbook = json(&schema);
    assert_eq!(runbook["type"], "runbook");
    assert_eq!(runbook["required"], serde_json::json!(["purpose", "steps"]));

    let template = run_in(&root, &["schema", "runbook", "--template"]);
    assert!(stdout(&template)
        .starts_with("# Title\n\n## Purpose\n\nTODO: describe what this runbook achieves."));
}

#[test]
fn golden_guard_no_bundle_means_exactly_the_builtins() {
    // A repository with a governing config but no `artifact_types` stanza, and
    // a directory with no config at all, both see the embedded five.
    let root = scratch("no-bundle");
    fs::create_dir_all(root.join(".decided")).unwrap();
    fs::write(
        root.join(".decided/config.yaml"),
        "repository_key: NB\ncorpus:\n  source: acme/no-bundle\n",
    )
    .unwrap();
    for cwd in [root.clone(), std::env::temp_dir()] {
        let output = run_in(&cwd, &["schema", "--list"]);
        assert!(output.status.success());
        assert_eq!(
            stdout(&output),
            "Available Schemas:\n- requirement\n- decision\n- roadmap\n- prompt\n- design\n"
        );
    }
    fs::create_dir_all(root.join("decisions")).unwrap();
    fs::write(
        root.join("decisions/adr.md"),
        "---\nschema_version: 1\nid: NB-000000000001\ntype: decision\n---\n# ADR-001: One\n\n## Status\n\nAccepted\n\n## Category\n\nOther\n\n## Context\n\nx\n\n## Decision\n\ny\n\n## Consequences\n\nz\n",
    )
    .unwrap();
    let output = run_in(&root, &["validate", "decisions", "--json"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let payload = json(&output);
    assert!(payload.get("artifact_spec_bundle").is_none());
    let stats = run_in(&root, &["stats", "decisions", "--json"]);
    let stats = json(&stats);
    assert!(stats.get("runbook").is_none());
}

#[test]
fn new_scaffolds_a_bundle_type_from_its_starter_bodies() {
    let root = fixture_copy("new");
    let created = run_in(
        &root,
        &[
            "new",
            "runbook",
            "decisions/runbooks/rotate-keys.md",
            "--json",
        ],
    );
    assert!(created.status.success(), "{}", stderr(&created));
    let payload = json(&created);
    assert_eq!(payload["type"], "runbook");
    let body = fs::read_to_string(root.join("decisions/runbooks/rotate-keys.md")).unwrap();
    assert!(body.starts_with("---\nschema_version: 1\nid: SPB-"));
    assert!(body.contains(
        "type: runbook\n---\n# Title\n\n## Purpose\n\nTODO: describe what this runbook achieves."
    ));
    assert!(body.contains("## Steps\n"));
    // The scaffold passes baseline validation for its own type (REQ-003).
    let validated = run_in(&root, &["validate", "decisions/runbooks/rotate-keys.md"]);
    assert!(validated.status.success(), "{}", stdout(&validated));

    // A type the bundle does not declare is still unknown to `new`.
    let unknown = run_in(&root, &["new", "playbook", "decisions/x.md"]);
    assert_eq!(unknown.status.code(), Some(2));
    assert!(stderr(&unknown).contains("playbook"));
}

#[test]
fn stats_exports_and_okf_carry_bundle_types() {
    let root = fixture();
    let stats = json(&run_in(&root, &["stats", "decisions", "--json"]));
    assert_eq!(stats["runbook"]["count"], 1);
    assert_eq!(stats["runbook"]["valid"], 1);
    assert_eq!(stats["policy"]["count"], 1);
    assert_eq!(stats["decisions"]["count"], 1);
    let human = stdout(&run_in(&root, &["stats", "decisions"]));
    assert!(human.contains("Runbooks\n========\n\nTotal: 1\nValid: 1"));
    assert!(human.contains("Policies\n========\n\nTotal: 1\nValid: 1"));

    let export = json(&run_in(&root, &["export", "decisions", "--json"]));
    let types: Vec<&str> = export["artifacts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a["type"].as_str().unwrap())
        .collect();
    assert_eq!(types, ["decision", "policy", "runbook"]);
    let runbook = &export["artifacts"][2];
    assert_eq!(runbook["status"], "Active");

    let out = scratch("okf");
    let okf = run_in(
        &root,
        &[
            "export",
            "decisions",
            "--okf",
            "--out",
            out.to_str().unwrap(),
        ],
    );
    assert!(okf.status.success(), "{}", stderr(&okf));
    let runbook_file = fs::read_to_string(out.join("runbooks/deploy-search-service.md")).unwrap();
    assert!(runbook_file.starts_with("---\ntype: Runbook\n"));
    let policy_file = fs::read_to_string(out.join("policies/data-retention.md")).unwrap();
    assert!(policy_file.starts_with("---\ntype: Policy\n"));
    let index = fs::read_to_string(out.join("index.md")).unwrap();
    let decisions_at = index.find("## Decisions").unwrap();
    let runbooks_at = index.find("## Runbooks").unwrap();
    let policies_at = index.find("## Policies").unwrap();
    assert!(decisions_at < runbooks_at && runbooks_at < policies_at);
}

#[test]
fn bundle_types_are_not_relationship_targets() {
    let root = fixture_copy("mismatch");
    fs::write(
        root.join("decisions/decisions/adr-002-ref.md"),
        "---\nschema_version: 1\nid: SPB-000000000004\ntype: decision\n---\n# ADR-002: References a Runbook\n\n## Status\n\nAccepted\n\n## Category\n\nProcess\n\n## Context\n\nx\n\n## Decision\n\ny\n\n## Consequences\n\nz\n\n## Related Decisions\n\n- deploy-search-service\n",
    )
    .unwrap();
    let output = run_in(
        &root,
        &["relationships", "decisions", "--validate", "--json"],
    );
    assert_eq!(output.status.code(), Some(1));
    let text = stdout(&output);
    assert!(text.contains("relationship-target-type-mismatch"), "{text}");
}

#[test]
fn digest_mismatch_is_a_hard_error_everywhere() {
    let root = fixture_copy("digest");
    let bundle = root.join(".decided/artifact-specs.json");
    let mut bytes = fs::read(&bundle).unwrap();
    bytes.push(b'\n');
    fs::write(&bundle, bytes).unwrap();

    // validate: one error row for the bundle, exit 1, in every output mode.
    let output = run_in(&root, &["validate", "decisions", "--json"]);
    assert_eq!(output.status.code(), Some(1));
    let payload = json(&output);
    assert_eq!(payload["valid"], false);
    let files = payload["files"].as_array().unwrap();
    assert_eq!(files.len(), 1);
    assert_eq!(files[0]["path"], ".decided/artifact-specs.json");
    assert_eq!(files[0]["artifact_type"], "artifact-spec-bundle");
    assert_eq!(files[0]["status"], "invalid");
    assert_eq!(
        files[0]["issues"][0]["code"],
        "artifact-spec-bundle-digest-mismatch"
    );
    let human = run_in(&root, &["validate", "decisions"]);
    assert_eq!(human.status.code(), Some(1));
    assert!(stdout(&human).contains("FAIL  .decided/artifact-specs.json  (artifact-spec-bundle)"));
    let sarif = run_in(&root, &["validate", "decisions", "--sarif"]);
    assert_eq!(sarif.status.code(), Some(1));
    assert!(stdout(&sarif).contains("artifact-spec-bundle-digest-mismatch"));

    // Every other command refuses in the federation shape.
    for args in [
        vec!["stats", "decisions"],
        vec!["find", "deploy", "decisions"],
        vec!["resolve", "SPB-000000000001", "decisions"],
        vec!["schema", "--list"],
        vec!["export", "decisions", "--json"],
        vec!["inspect", "decisions/runbooks/deploy-search-service.md"],
        vec!["new", "runbook", "decisions/runbooks/again.md"],
    ] {
        let output = run_in(&root, &args);
        assert_eq!(output.status.code(), Some(1), "{args:?}");
        assert!(
            stderr(&output).starts_with("decided: artifact-spec-bundle-digest-mismatch: "),
            "{args:?}: {}",
            stderr(&output)
        );
        assert!(stdout(&output).is_empty(), "{args:?}");
    }
}

#[test]
fn malformed_stanza_and_missing_bundle_are_hard_errors() {
    let root = fixture_copy("stanza");
    fs::write(
        root.join(".decided/config.yaml"),
        "repository_key: SPB\nartifact_types:\n  version: 2\n  bundle:\n    path: .decided/artifact-specs.json\n    digest: sha256:00\n",
    )
    .unwrap();
    let output = run_in(&root, &["stats", "decisions"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(stderr(&output).starts_with("decided: artifact-spec-bundle-config-invalid: "));

    fs::write(
        root.join(".decided/config.yaml"),
        format!(
            "repository_key: SPB\nartifact_types:\n  version: 1\n  bundle:\n    path: .decided/absent.json\n    digest: sha256:{}\n",
            "0".repeat(64)
        ),
    )
    .unwrap();
    let output = run_in(&root, &["validate", "decisions", "--json"]);
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(
        json(&output)["files"][0]["issues"][0]["code"],
        "artifact-spec-bundle-missing"
    );
}

#[test]
fn doctor_reports_skipped_elements_as_warnings() {
    let root = fixture();
    let output = run_in(&root, &["doctor", "decisions", "--json"]);
    let payload = json(&output);
    let findings = payload["findings"].as_array().unwrap();
    let skipped: Vec<&serde_json::Value> = findings
        .iter()
        .filter(|f| f["code"] == "artifact-spec-skipped")
        .collect();
    assert_eq!(skipped.len(), 1);
    assert_eq!(skipped[0]["severity"], "warning");
    assert_eq!(skipped[0]["path"], ".decided/artifact-specs.json");
    assert!(skipped[0]["problem"]
        .as_str()
        .unwrap()
        .contains("collides with a built-in artifact type"));
}

#[test]
fn a_bundle_type_does_not_misclassify_as_a_builtin_and_vice_versa() {
    let root = fixture_copy("boundary");
    // Prompt-like headings under a runbook frontmatter: the prompt spec wins
    // on fit, and the frontmatter/classification disagreement surfaces as a
    // validation finding rather than a silent runbook.
    fs::write(
        root.join("decisions/runbooks/looks-like-a-prompt.md"),
        "---\nschema_version: 1\nid: SPB-000000000009\ntype: runbook\n---\n# Not a Runbook\n\n## Objective\n\nx\n\n## Input\n\ny\n\n## Instructions\n\nz\n\n## Output\n\nw\n",
    )
    .unwrap();
    let inspect = json(&run_in(
        &root,
        &[
            "inspect",
            "decisions/runbooks/looks-like-a-prompt.md",
            "--json",
        ],
    ));
    assert_eq!(inspect["type"], "prompt");
    // And a genuine runbook is a runbook, not the nearest built-in.
    let inspect = json(&run_in(
        &root,
        &[
            "inspect",
            "decisions/runbooks/deploy-search-service.md",
            "--json",
        ],
    ));
    assert_eq!(inspect["type"], "runbook");
    assert_eq!(inspect["confidence"], 1.0);
}

/// Break the fixture runbook (drop its required `## Steps`).
fn drop_runbook_steps(root: &Path) {
    let path = root.join("decisions/runbooks/deploy-search-service.md");
    let text = fs::read_to_string(&path).unwrap();
    let start = text.find("## Steps").unwrap();
    let end = text.find("## Rollback").unwrap();
    fs::write(&path, format!("{}{}", &text[..start], &text[end..])).unwrap();
}

/// Change the bundle bytes without re-pinning them.
fn tamper_bundle(root: &Path) {
    let path = root.join(".decided/artifact-specs.json");
    let mut bytes = fs::read(&path).unwrap();
    bytes.extend_from_slice(b" \n");
    fs::write(path, bytes).unwrap();
}

#[test]
fn gate_sentry_and_rename_load_the_bundle_and_refuse_a_broken_pin() {
    let root = fixture_copy("every-command");
    drop_runbook_steps(&root);
    // The gate sees what validate sees: the runbook is a runbook, and invalid.
    let gate = run_in(&root, &["gate", "decisions", "--json"]);
    assert_eq!(gate.status.code(), Some(1), "{}", stdout(&gate));
    assert!(stdout(&gate).contains("missing-steps"), "{}", stdout(&gate));

    tamper_bundle(&root);
    for args in [
        ["gate", "decisions"].as_slice(),
        ["sentry", "decisions", "--full"].as_slice(),
        [
            "rename",
            "SPB-000000000003",
            "SPB-000000000099",
            "decisions",
        ]
        .as_slice(),
        ["watchkeeper", "decisions", "--base", "decisions"].as_slice(),
    ] {
        let output = run_in(&root, args);
        assert_eq!(
            output.status.code(),
            Some(1),
            "{args:?}: {}",
            stdout(&output)
        );
        assert!(
            stderr(&output).starts_with("decided: artifact-spec-bundle-digest-mismatch: "),
            "{args:?}: {}",
            stderr(&output)
        );
    }
}

#[test]
fn rename_rewrites_references_held_by_bundle_type_artifacts() {
    let root = fixture_copy("rename");
    for relative in [
        "decisions/runbooks/deploy-search-service.md",
        "decisions/policies/data-retention.md",
    ] {
        let path = root.join(relative);
        let text = fs::read_to_string(&path)
            .unwrap()
            .replace("- adr-001-example\n", "- SPB-000000000003\n");
        fs::write(path, text).unwrap();
    }
    let applied = run_in(
        &root,
        &[
            "rename",
            "SPB-000000000003",
            "SPB-000000000099",
            "decisions",
            "--apply",
        ],
    );
    assert_eq!(applied.status.code(), Some(0), "{}", stderr(&applied));
    assert!(
        stdout(&applied).contains("Applied: 2 reference(s)"),
        "{}",
        stdout(&applied)
    );
    let check = run_in(&root, &["relationships", "decisions", "--validate"]);
    assert_eq!(check.status.code(), Some(0), "{}", stdout(&check));
    for relative in [
        "decisions/runbooks/deploy-search-service.md",
        "decisions/policies/data-retention.md",
    ] {
        assert!(fs::read_to_string(root.join(relative))
            .unwrap()
            .contains("- SPB-000000000099"));
    }
}

#[test]
fn watchkeeper_compares_bundle_types_under_the_working_tree_registry() {
    let root = fixture_copy("watchkeeper");
    let base = scratch("watchkeeper-base");
    copy_dir(&root.join("decisions"), &base);
    drop_runbook_steps(&root);
    let output = run_in(
        &root,
        &["watchkeeper", "decisions", "--base", base.to_str().unwrap()],
    );
    let text = stdout(&output);
    assert!(
        text.contains("runbooks/deploy-search-service.md  (runbook)"),
        "{text}"
    );
    assert!(text.contains("Invalid:  0 → 1"), "{text}");
}

#[test]
fn stdin_inspect_and_improve_see_the_bundle_types() {
    let root = fixture();
    let runbook = fs::read(root.join("decisions/runbooks/deploy-search-service.md")).unwrap();
    for command in ["inspect", "improve"] {
        let mut child = Command::new(env!("CARGO_BIN_EXE_decided"))
            .args([command, "-"])
            .current_dir(&root)
            .env("DECIDED_NO_CACHE", "1")
            .env("LC_ALL", "C")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        {
            use std::io::Write;
            child.stdin.take().unwrap().write_all(&runbook).unwrap();
        }
        let output = child.wait_with_output().unwrap();
        assert!(
            stdout(&output).starts_with("Artifact Type: Runbook"),
            "{command}: {}",
            stdout(&output)
        );
    }
}

#[test]
fn an_unfederated_historical_export_uses_the_bundle_of_that_revision() {
    let root = fixture_copy("export-at");
    let git = |args: &[&str]| {
        let status = Command::new("git")
            .args(["-c", "user.name=t", "-c", "user.email=t@example.invalid"])
            .args(args)
            .current_dir(&root)
            .status()
            .unwrap();
        assert!(status.success(), "git {args:?}");
    };
    git(&["init", "-q"]);
    git(&["add", "-A"]);
    git(&["commit", "-qm", "one"]);
    // Commit two re-pins a bundle without `runbook`.
    let path = root.join(".decided/artifact-specs.json");
    let mut bundle: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    bundle["artifact_specs"]
        .as_array_mut()
        .unwrap()
        .retain(|e| e["name"] != "runbook");
    let bytes = format!("{}\n", serde_json::to_string_pretty(&bundle).unwrap());
    fs::write(&path, &bytes).unwrap();
    let config = root.join(".decided/config.yaml");
    let text = fs::read_to_string(&config).unwrap();
    let start = text.find("digest: sha256:").unwrap();
    let end = start + "digest: sha256:".len() + 64;
    let digest = {
        use sha2::{Digest, Sha256};
        format!("{:x}", Sha256::digest(bytes.as_bytes()))
    };
    fs::write(
        &config,
        format!("{}digest: sha256:{digest}{}", &text[..start], &text[end..]),
    )
    .unwrap();
    git(&["commit", "-qam", "two"]);

    let types = |rev: &str| -> Vec<String> {
        let output = run_in(&root, &["export", "decisions", "--at", rev, "--json"]);
        assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
        let mut types: Vec<String> = json(&output)["artifacts"]
            .as_array()
            .unwrap()
            .iter()
            .map(|a| a["type"].as_str().unwrap().to_string())
            .collect();
        types.sort();
        types.dedup();
        types
    };
    assert_eq!(types("HEAD~1"), ["decision", "policy", "runbook"]);
    assert_eq!(types("HEAD"), ["decision", "policy"]);
}

/// Append `element` to the fixture bundle and re-pin it.
fn add_element_and_repin(root: &Path, element: &str) {
    let path = root.join(".decided/artifact-specs.json");
    let mut bundle: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    bundle["artifact_specs"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::from_str(element).unwrap());
    let bytes = format!("{}\n", serde_json::to_string_pretty(&bundle).unwrap());
    fs::write(&path, &bytes).unwrap();
    let config = root.join(".decided/config.yaml");
    let text = fs::read_to_string(&config).unwrap();
    let start = text.find("digest: sha256:").unwrap();
    let end = start + "digest: sha256:".len() + 64;
    let digest = {
        use sha2::{Digest, Sha256};
        format!("{:x}", Sha256::digest(bytes.as_bytes()))
    };
    fs::write(
        &config,
        format!("{}digest: sha256:{digest}{}", &text[..start], &text[end..]),
    )
    .unwrap();
}

#[test]
fn a_bundle_type_cannot_take_classification_from_a_builtin() {
    let root = fixture_copy("builtins-win");
    // `note` requires only `context`, which every ADR carries: it scores 1.0
    // on the fixture ADR, which lacks a recommended section. Built-ins win.
    add_element_and_repin(
        &root,
        r#"{"name":"note","display":"Note","required":["context"]}"#,
    );
    let inspected = run_in(
        &root,
        &["inspect", "decisions/decisions/adr-001-example.md"],
    );
    assert!(
        stdout(&inspected).starts_with("Artifact Type: Decision"),
        "{}",
        stdout(&inspected)
    );
    let payload = json(&run_in(&root, &["validate", "decisions", "--json"]));
    let adr = payload["files"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["path"] == "decisions/decisions/adr-001-example.md")
        .unwrap();
    assert_eq!(adr["artifact_type"], "decision");
    // A document only the bundle type fits is still that type.
    fs::create_dir_all(root.join("decisions/notes")).unwrap();
    fs::write(
        root.join("decisions/notes/n.md"),
        "---\nschema_version: 1\nid: SPB-000000000007\ntype: note\n---\n# A Note\n\n## Context\n\nOnly context.\n",
    )
    .unwrap();
    let inspected = run_in(&root, &["inspect", "decisions/notes/n.md"]);
    assert!(
        stdout(&inspected).starts_with("Artifact Type: Note"),
        "{}",
        stdout(&inspected)
    );
}

#[test]
fn an_unparseable_config_with_a_stanza_fails_instead_of_dropping_the_pin() {
    let root = fixture_copy("unparseable");
    let config = root.join(".decided/config.yaml");
    let mut text = fs::read_to_string(&config).unwrap();
    text.push_str("extra: [unclosed\n");
    fs::write(&config, text).unwrap();
    let output = run_in(&root, &["validate", "decisions", "--json"]);
    assert_eq!(output.status.code(), Some(1), "{}", stdout(&output));
    assert_eq!(
        json(&output)["files"][0]["issues"][0]["code"],
        "artifact-spec-bundle-config-invalid"
    );
}

#[test]
fn okf_types_are_emitted_as_safe_scalars() {
    let root = fixture_copy("okf-scalar");
    let path = root.join(".decided/artifact-specs.json");
    let mut bundle: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    bundle["artifact_specs"][0]["okf_type"] = serde_json::json!("Run Book: Ops");
    let bytes = format!("{}\n", serde_json::to_string_pretty(&bundle).unwrap());
    fs::write(&path, &bytes).unwrap();
    let config = root.join(".decided/config.yaml");
    let text = fs::read_to_string(&config).unwrap();
    let start = text.find("digest: sha256:").unwrap();
    let end = start + "digest: sha256:".len() + 64;
    let digest = {
        use sha2::{Digest, Sha256};
        format!("{:x}", Sha256::digest(bytes.as_bytes()))
    };
    fs::write(
        &config,
        format!("{}digest: sha256:{digest}{}", &text[..start], &text[end..]),
    )
    .unwrap();
    let out = root.join("okf-out");
    let export = run_in(
        &root,
        &[
            "export",
            "decisions",
            "--okf",
            "--out",
            out.to_str().unwrap(),
        ],
    );
    assert_eq!(export.status.code(), Some(0), "{}", stderr(&export));
    let runbook = fs::read_to_string(out.join("runbooks/deploy-search-service.md"))
        .or_else(|_| fs::read_to_string(out.join("decisions/runbooks/deploy-search-service.md")))
        .unwrap();
    assert!(runbook.contains("type: \"Run Book: Ops\"\n"), "{runbook}");
    let policy = fs::read_to_string(out.join("policies/data-retention.md"))
        .or_else(|_| fs::read_to_string(out.join("decisions/policies/data-retention.md")))
        .unwrap();
    assert!(policy.contains("type: Policy\n"), "{policy}");
}
