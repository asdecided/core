//! Inherited spec bundles across the federation graph (ADR-150): the
//! `rust/fixtures/spec-federation/` corpus is a version-2 child pinning one
//! parent (`standards`) that declares `runbook`; the child declares `policy`.
//! These tests pin the effective registry's behaviour end to end through the
//! CLI, the collision and override rules, the parent-side pin check, and the
//! golden guard: a federated closure in which no source pins a bundle is
//! unchanged.

#[allow(dead_code)]
mod federation_support;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use federation_support::{FederationRepo, CHILD_SOURCE, PARENT_SOURCE};

const CHILD: &str = "asdecided/fixtures-spec-child";
const STANDARDS: &str = "asdecided/fixtures-spec-standards";
const LIVE_RATIONALE: &str = "SPC-000000000002";
const PROPOSED_RATIONALE: &str = "SPC-000000000003";
const POLICY_ID: &str = "SPC-000000000001";
const CONFLICT: &str = "corpus-federation-artifact-type-conflict";
const INVALID_OVERRIDE: &str = "corpus-federation-invalid-override";

/// A `runbook` element that differs from the standards declaration.
const RUNBOOK_ALT: &str = r#"{"name":"runbook","display":"Run Book","required":["purpose"],"recommended":[],"optional":[],"metadata":{"status":["Active"]},"retired_status":[],"descriptions":{},"guidance":{},"synonyms":{},"id_field":null,"starter_bodies":{"purpose":"TODO"}}"#;

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../fixtures/spec-federation")
        .canonicalize()
        .expect("spec-federation fixture exists")
}

fn eval_fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../fixtures/eval/federation")
        .canonicalize()
        .expect("eval federation fixture exists")
}

fn scratch(tag: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "asdecided-spec-federation-{tag}-{}-{}",
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

fn sha256(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

/// Replace the bundle at `bundle` with `elements` and re-pin it in `config`.
fn write_and_repin(root: &Path, bundle: &str, config: &str, elements: &[&str]) {
    let json = format!("{{\"artifact_specs\":[{}]}}\n", elements.join(","));
    fs::write(root.join(bundle), &json).unwrap();
    let path = root.join(config);
    let text = fs::read_to_string(&path).unwrap();
    let start = text.find("digest: sha256:").expect("config pins a bundle");
    let end = start + "digest: sha256:".len() + 64;
    let repinned = format!(
        "{}digest: sha256:{}{}",
        &text[..start],
        sha256(json.as_bytes()),
        &text[end..]
    );
    fs::write(path, repinned).unwrap();
}

/// The standards runbook element exactly as the parent bundle declares it.
fn standards_runbook(root: &Path) -> String {
    let bundle: serde_json::Value = serde_json::from_slice(
        &fs::read(root.join("vendor/standards/.decided/artifact-specs.json")).unwrap(),
    )
    .unwrap();
    bundle["artifact_specs"][0].to_string()
}

fn append_overrides(root: &Path, entries: &[(&str, &str, &str)]) {
    let path = root.join(".decided/config.yaml");
    let mut text = fs::read_to_string(&path).unwrap();
    text.push_str("  overrides:\n");
    for (name, prefer, rationale) in entries {
        text.push_str(&format!(
            "    - name: {name}\n      prefer: {prefer}\n      rationale: {rationale}\n"
        ));
    }
    fs::write(path, text).unwrap();
}

fn manifest_failure(output: &Output, code: &str) -> serde_json::Value {
    assert_eq!(output.status.code(), Some(1), "{}", stderr(output));
    let payload = json(output);
    assert_eq!(payload["valid"], false);
    let row = &payload["files"][0];
    assert_eq!(row["path"], ".decided/corpus.md");
    assert_eq!(row["artifact_type"], "corpus-manifest");
    assert_eq!(row["issues"][0]["code"], code, "{row}");
    row.clone()
}

#[test]
fn a_child_inherits_the_parent_runbook_type_with_per_source_provenance() {
    let root = fixture();
    let output = run_in(&root, &["validate", "decisions", "--json"]);
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    let payload = json(&output);
    assert_eq!(payload["valid"], true);
    let runbook = payload["files"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["path"] == "runbooks/deploy-search-service.md")
        .expect("the parent's runbook is validated in the child");
    assert_eq!(runbook["artifact_type"], "runbook");
    assert_eq!(runbook["status"], "valid");
    assert_eq!(runbook["provenance"]["source"], STANDARDS);
    assert_eq!(runbook["provenance"]["layer"], "inherited");

    // The ADR-083 key stays for the local bundle; the ADR-150 list carries
    // every contributing source in composition order.
    assert_eq!(
        payload["artifact_spec_bundle"]["admitted"],
        serde_json::json!(["policy"])
    );
    let bundles = payload["artifact_spec_bundles"].as_array().unwrap();
    let summary: Vec<(String, String, Vec<String>)> = bundles
        .iter()
        .map(|b| {
            (
                b["source"].as_str().unwrap().to_string(),
                b["layer"].as_str().unwrap().to_string(),
                b["admitted"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| v.as_str().unwrap().to_string())
                    .collect(),
            )
        })
        .collect();
    assert_eq!(
        summary,
        [
            (
                CHILD.to_string(),
                "local".to_string(),
                vec!["policy".to_string()]
            ),
            (
                STANDARDS.to_string(),
                "inherited".to_string(),
                vec!["runbook".to_string()]
            ),
        ]
    );
    assert!(bundles[1]["digest"]
        .as_str()
        .unwrap()
        .starts_with("sha256:"));
    assert_eq!(bundles[1]["warnings"], serde_json::json!([]));

    let human = run_in(&root, &["validate", "decisions"]);
    assert_eq!(human.status.code(), Some(0));
    assert!(
        stdout(&human).starts_with("PASS  decisions"),
        "{}",
        stdout(&human)
    );

    let stats = json(&run_in(&root, &["stats", "decisions", "--json"]));
    assert_eq!(stats["runbook"]["count"], 1);
    assert_eq!(stats["policy"]["count"], 1);

    let export = json(&run_in(&root, &["export", "decisions", "--json"]));
    let inherited_runbook = export["artifacts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["type"] == "runbook")
        .expect("the export carries the inherited runbook");
    assert_eq!(inherited_runbook["provenance"]["source"], STANDARDS);
}

#[test]
fn golden_guard_a_closure_without_bundles_is_unchanged() {
    let root = eval_fixture();
    let output = run_in(&root, &["validate", "graph-decisions", "--json"]);
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    let payload = json(&output);
    assert!(payload.get("artifact_spec_bundle").is_none());
    assert!(payload.get("artifact_spec_bundles").is_none());
    let keys: Vec<&str> = payload
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(
        keys,
        [
            "schema_version",
            "directory",
            "recursive",
            "summary",
            "valid",
            "files",
            "okf"
        ]
    );
    let stats = json(&run_in(&root, &["stats", "graph-decisions", "--json"]));
    assert!(stats.get("runbook").is_none());
}

#[test]
fn identical_declarations_by_child_and_parent_are_silent() {
    let root = fixture_copy("identical");
    let runbook = standards_runbook(&root);
    let policy: serde_json::Value =
        serde_json::from_slice(&fs::read(root.join(".decided/artifact-specs.json")).unwrap())
            .unwrap();
    let policy = policy["artifact_specs"][0].to_string();
    write_and_repin(
        &root,
        ".decided/artifact-specs.json",
        ".decided/config.yaml",
        &[&policy, &runbook],
    );
    let output = run_in(&root, &["validate", "decisions", "--json"]);
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    let payload = json(&output);
    assert_eq!(payload["valid"], true);
    assert_eq!(
        payload["artifact_spec_bundle"]["admitted"],
        serde_json::json!(["policy", "runbook"])
    );
    assert_eq!(
        payload["artifact_spec_bundles"][1]["admitted"],
        serde_json::json!(["runbook"])
    );
    let stats = json(&run_in(&root, &["stats", "decisions", "--json"]));
    assert_eq!(stats["runbook"]["count"], 1);
}

#[test]
fn different_content_for_one_name_is_a_composition_error_everywhere() {
    let root = fixture_copy("conflict");
    let policy: serde_json::Value =
        serde_json::from_slice(&fs::read(root.join(".decided/artifact-specs.json")).unwrap())
            .unwrap();
    let policy = policy["artifact_specs"][0].to_string();
    write_and_repin(
        &root,
        ".decided/artifact-specs.json",
        ".decided/config.yaml",
        &[&policy, RUNBOOK_ALT],
    );

    let row = manifest_failure(
        &run_in(&root, &["validate", "decisions", "--json"]),
        CONFLICT,
    );
    let message = row["issues"][0]["message"].as_str().unwrap();
    assert!(message.contains("'runbook'"), "{message}");
    assert!(
        message.contains(&format!("'{CHILD}' and '{STANDARDS}'")),
        "{message}"
    );
    assert_eq!(row["provenance"]["source"], CHILD);

    let human = run_in(&root, &["validate", "decisions"]);
    assert_eq!(human.status.code(), Some(1));
    assert!(
        stdout(&human).contains(&format!("[{CONFLICT}]")),
        "{}",
        stdout(&human)
    );
    let sarif = run_in(&root, &["validate", "decisions", "--sarif"]);
    assert!(stdout(&sarif).contains(CONFLICT));

    for args in [
        ["stats", "decisions"].as_slice(),
        ["find", "deploy", "decisions"].as_slice(),
        ["export", "decisions", "--json"].as_slice(),
        ["doctor", "decisions"].as_slice(),
    ] {
        let output = run_in(&root, args);
        assert_eq!(
            output.status.code(),
            Some(1),
            "{args:?}: {}",
            stderr(&output)
        );
        assert!(
            stderr(&output).starts_with(&format!("decided: {CONFLICT}: ")),
            "{args:?}: {}",
            stderr(&output)
        );
    }
}

#[test]
fn a_decision_backed_override_selects_the_preferred_declaration() {
    let root = fixture_copy("override");
    let policy: serde_json::Value =
        serde_json::from_slice(&fs::read(root.join(".decided/artifact-specs.json")).unwrap())
            .unwrap();
    let policy = policy["artifact_specs"][0].to_string();
    write_and_repin(
        &root,
        ".decided/artifact-specs.json",
        ".decided/config.yaml",
        &[&policy, RUNBOOK_ALT],
    );
    append_overrides(&root, &[("runbook", "local", LIVE_RATIONALE)]);

    let output = run_in(&root, &["validate", "decisions", "--json"]);
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    let payload = json(&output);
    assert_eq!(payload["valid"], true);
    let runbook = payload["files"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["path"] == "runbooks/deploy-search-service.md")
        .unwrap();
    assert_eq!(runbook["artifact_type"], "runbook");
    // The local declaration governs the whole closure, the parent's included.
    let schema = run_in(&root, &["validate", "decisions", "--json"]);
    assert_eq!(schema.status.code(), Some(0));
    let stats = json(&run_in(&root, &["stats", "decisions", "--json"]));
    assert_eq!(stats["runbook"]["count"], 1);
    let export = json(&run_in(&root, &["export", "decisions", "--json"]));
    let runbook = export["artifacts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["type"] == "runbook")
        .unwrap();
    assert_eq!(runbook["provenance"]["source"], STANDARDS);

    // Preferring the parent keeps its declaration instead.
    let config = root.join(".decided/config.yaml");
    let text = fs::read_to_string(&config)
        .unwrap()
        .replace("prefer: local", &format!("prefer: {STANDARDS}"));
    fs::write(&config, text).unwrap();
    let output = run_in(&root, &["validate", "decisions", "--json"]);
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    assert_eq!(json(&output)["valid"], true);
}

#[test]
fn override_defects_fail_with_one_stable_code_each() {
    let policy_of = |root: &Path| -> String {
        let policy: serde_json::Value =
            serde_json::from_slice(&fs::read(root.join(".decided/artifact-specs.json")).unwrap())
                .unwrap();
        policy["artifact_specs"][0].to_string()
    };
    let cases: [(&str, &[(&str, &str, &str)], &str, &str); 6] = [
        (
            "proposed",
            &[("runbook", "local", PROPOSED_RATIONALE)],
            INVALID_OVERRIDE,
            "is not a live Decision",
        ),
        (
            "missing",
            &[("runbook", "local", "SPC-000000000099")],
            INVALID_OVERRIDE,
            "does not resolve to a local artifact",
        ),
        (
            "not-a-decision",
            &[("runbook", "local", POLICY_ID)],
            INVALID_OVERRIDE,
            "is not a Decision",
        ),
        (
            "outside-view",
            &[("runbook", "acme/elsewhere", LIVE_RATIONALE)],
            INVALID_OVERRIDE,
            "is not a parent in the inherited view",
        ),
        (
            "no-collision",
            &[
                ("runbook", "local", LIVE_RATIONALE),
                ("policy", "local", LIVE_RATIONALE),
            ],
            INVALID_OVERRIDE,
            "does not collide",
        ),
        (
            "duplicate-name",
            &[
                ("runbook", "local", LIVE_RATIONALE),
                ("runbook", STANDARDS, LIVE_RATIONALE),
            ],
            "artifact-spec-bundle-config-invalid",
            "more than once",
        ),
    ];
    for (tag, overrides, code, fragment) in cases {
        let root = fixture_copy(&format!("defect-{tag}"));
        let policy = policy_of(&root);
        write_and_repin(
            &root,
            ".decided/artifact-specs.json",
            ".decided/config.yaml",
            &[&policy, RUNBOOK_ALT],
        );
        append_overrides(&root, overrides);
        let output = run_in(&root, &["validate", "decisions", "--json"]);
        assert_eq!(output.status.code(), Some(1), "{tag}: {}", stderr(&output));
        let payload = json(&output);
        let row = &payload["files"][0];
        assert_eq!(row["issues"][0]["code"], code, "{tag}: {row}");
        let message = row["issues"][0]["message"].as_str().unwrap();
        assert!(message.contains(fragment), "{tag}: {message}");
    }
}

#[test]
fn a_parent_bundle_edited_without_a_repin_fails_closed_with_its_source() {
    let root = fixture_copy("tamper");
    let path = root.join("vendor/standards/.decided/artifact-specs.json");
    let mut bundle: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    bundle["artifact_specs"][0]["display"] = serde_json::json!("Tampered");
    fs::write(&path, serde_json::to_string_pretty(&bundle).unwrap()).unwrap();

    let row = manifest_failure(
        &run_in(&root, &["validate", "decisions", "--json"]),
        "artifact-spec-bundle-digest-mismatch",
    );
    let message = row["issues"][0]["message"].as_str().unwrap();
    assert!(
        message.contains(&format!("inherited source '{STANDARDS}'")),
        "{message}"
    );
    assert_eq!(row["provenance"]["source"], STANDARDS);
    assert_eq!(row["provenance"]["layer"], "inherited");
    assert_eq!(
        row["provenance"]["source_route"],
        serde_json::json!([CHILD, STANDARDS])
    );
    let output = run_in(&root, &["stats", "decisions"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(stderr(&output).starts_with("decided: artifact-spec-bundle-digest-mismatch: "));
}

#[test]
fn overrides_in_an_unfederated_corpus_are_rejected() {
    let root = scratch("unfederated");
    copy_dir(
        &Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../fixtures/spec-bundle")
            .canonicalize()
            .unwrap(),
        &root,
    );
    append_overrides(&root, &[("runbook", "local", "SPB-000000000003")]);
    let output = run_in(&root, &["stats", "decisions"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(
        stderr(&output).starts_with(&format!("decided: {INVALID_OVERRIDE}: ")),
        "{}",
        stderr(&output)
    );
    assert!(stderr(&output).contains("declares no parents"));
    let output = run_in(&root, &["validate", "decisions", "--json"]);
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(
        json(&output)["files"][0]["issues"][0]["code"],
        INVALID_OVERRIDE
    );

    // An unknown key under the stanza is a config error, as under `bundle`.
    let config = root.join(".decided/config.yaml");
    let text = fs::read_to_string(&config)
        .unwrap()
        .replace("  overrides:\n", "  extra: 1\n  overrides:\n");
    fs::write(&config, text).unwrap();
    let output = run_in(&root, &["stats", "decisions"]);
    assert!(stderr(&output).starts_with("decided: artifact-spec-bundle-config-invalid: "));
}

#[test]
fn new_scaffolds_inherited_and_builtin_types_in_a_version_two_child() {
    // The version-2 graph rejects the repository root as a corpus path, so
    // the scaffold's identifier-collision scan composes the top-level corpus
    // directory holding the target instead (local and inherited layers).
    let root = fixture_copy("new-v2");
    fs::create_dir_all(root.join("decisions/runbooks")).unwrap();
    let created = run_in(
        &root,
        &["new", "runbook", "decisions/runbooks/rotate-keys.md"],
    );
    assert_eq!(created.status.code(), Some(0), "{}", stderr(&created));
    let body = fs::read_to_string(root.join("decisions/runbooks/rotate-keys.md")).unwrap();
    assert!(body.contains("type: runbook"), "{body}");
    assert!(body.contains("## Purpose"), "{body}");
    let created = run_in(
        &root,
        &[
            "new",
            "decision",
            "decisions/decisions/adr-004-new.md",
            "--json",
        ],
    );
    assert_eq!(created.status.code(), Some(0), "{}", stderr(&created));
    let id = json(&created)["id"].as_str().unwrap().to_string();
    assert!(id.starts_with("SPC-"), "{id}");

    let corpus = json(&run_in(&root, &["validate", "decisions", "--json"]));
    assert_eq!(corpus["valid"], true, "{corpus}");
    let rows: Vec<(&str, &str)> = corpus["files"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|f| {
            f["path"] == "runbooks/rotate-keys.md" || f["path"] == "decisions/adr-004-new.md"
        })
        .map(|f| {
            (
                f["artifact_type"].as_str().unwrap(),
                f["provenance"]["source"].as_str().unwrap(),
            )
        })
        .collect();
    assert_eq!(rows, [("decision", CHILD), ("runbook", CHILD)]);

    // A missing target directory is still the released usage error, and a
    // target inside the read-only parent is still refused.
    let missing = run_in(&root, &["new", "runbook", "decisions/absent/x.md"]);
    assert_eq!(missing.status.code(), Some(2));
    assert!(stderr(&missing).contains("directory does not exist"));
    let refused = run_in(
        &root,
        &["new", "runbook", "vendor/standards/decisions/runbooks/x.md"],
    );
    assert_eq!(refused.status.code(), Some(1));
    assert!(
        stderr(&refused).contains("read-only"),
        "{}",
        stderr(&refused)
    );

    // A version-2 root with no bundle anywhere scaffolds a built-in too.
    let plain = scratch("new-v2-plain");
    copy_dir(&eval_fixture(), &plain);
    let created = run_in(
        &plain,
        &["new", "decision", "graph-decisions/new-decision.md"],
    );
    assert_eq!(created.status.code(), Some(0), "{}", stderr(&created));
    let validated = run_in(&plain, &["validate", "graph-decisions"]);
    assert_eq!(validated.status.code(), Some(0), "{}", stderr(&validated));
}

#[test]
fn schema_and_templates_list_the_effective_registry_of_a_given_corpus() {
    let root = fixture();
    // Without a corpus the working directory's local registry is listed, as
    // released: the child's own bundle, nothing inherited.
    let local = json(&run_in(&root, &["schema", "--list", "--json"]));
    let names: Vec<&str> = local["schemas"]
        .as_array()
        .unwrap()
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
            "policy"
        ]
    );
    let absent = run_in(&root, &["schema", "runbook"]);
    assert_eq!(absent.status.code(), Some(2));

    // With --corpus the composed closure is listed: own types, then inherited.
    let effective = json(&run_in(
        &root,
        &["schema", "--list", "--json", "--corpus", "decisions"],
    ));
    let names: Vec<&str> = effective["schemas"]
        .as_array()
        .unwrap()
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
            "policy",
            "runbook"
        ]
    );
    let inherited = run_in(&root, &["schema", "runbook", "--corpus=decisions"]);
    assert_eq!(inherited.status.code(), Some(0), "{}", stderr(&inherited));
    assert!(
        stdout(&inherited).contains("Runbook"),
        "{}",
        stdout(&inherited)
    );
    let template = run_in(
        &root,
        &["schema", "runbook", "--template", "--corpus", "decisions"],
    );
    assert_eq!(template.status.code(), Some(0));
    assert!(
        stdout(&template).contains("## Purpose"),
        "{}",
        stdout(&template)
    );
    let templates = json(&run_in(
        &root,
        &["templates", "--json", "--corpus", "decisions"],
    ));
    assert_eq!(
        templates["templates"].as_array().unwrap().last().unwrap(),
        "runbook"
    );

    // The repository root of a version-2 graph is not a corpus path: it is
    // refused exactly as `validate <root>` refuses it, never silently listed
    // without its inherited types.
    let root_listing = run_in(&root, &["schema", "--list", "--corpus", "."]);
    assert_eq!(
        root_listing.status.code(),
        Some(1),
        "{}",
        stdout(&root_listing)
    );
    assert!(
        stderr(&root_listing).starts_with("decided: federated-corpus-snapshot-failed: "),
        "{}",
        stderr(&root_listing)
    );
    let root_validate = run_in(&root, &["validate", "."]);
    assert_eq!(root_validate.status.code(), Some(1));
    assert!(stdout(&root_validate).contains("federated-corpus-snapshot-failed"));

    // A directory that is not a corpus is a usage error; a closure that
    // cannot be composed is the composition failure, exit 1.
    let missing = run_in(&root, &["templates", "--corpus", "absent"]);
    assert_eq!(missing.status.code(), Some(2));
    assert!(stderr(&missing).contains("--corpus is not a directory"));
    let conflict = fixture_copy("schema-conflict");
    let policy: serde_json::Value =
        serde_json::from_slice(&fs::read(conflict.join(".decided/artifact-specs.json")).unwrap())
            .unwrap();
    let policy = policy["artifact_specs"][0].to_string();
    write_and_repin(
        &conflict,
        ".decided/artifact-specs.json",
        ".decided/config.yaml",
        &[&policy, RUNBOOK_ALT],
    );
    let failed = run_in(&conflict, &["schema", "--list", "--corpus", "decisions"]);
    assert_eq!(failed.status.code(), Some(1));
    assert!(
        stderr(&failed).starts_with(&format!("decided: {CONFLICT}: ")),
        "{}",
        stderr(&failed)
    );
}

#[test]
fn a_version_one_parent_bundle_is_inherited_and_scaffolds_in_the_child() {
    let repo = FederationRepo::new("spec-bundle-v-one");
    let runbook = standards_runbook(&fixture());
    let bundle = format!("{{\"artifact_specs\":[{runbook}]}}\n");
    repo.write("vendor/standards/.decided/artifact-specs.json", &bundle);
    repo.write(
        "vendor/standards/.decided/config.yaml",
        &format!(
            "repository_key: STD\ncorpus:\n  source: {PARENT_SOURCE}\nartifact_types:\n  version: 1\n  bundle:\n    path: .decided/artifact-specs.json\n    digest: sha256:{}\n",
            sha256(bundle.as_bytes())
        ),
    );
    repo.write(
        "vendor/standards/decisions/runbooks/deploy.md",
        "---\nschema_version: 1\nid: STD-000000000009\ntype: runbook\n---\n# Deploy\n\n## Status\n\nActive\n\n## Purpose\n\nRoll a build.\n\n## Steps\n\n1. Drain.\n2. Deploy.\n",
    );
    repo.activate();

    let output = repo.run(&["validate", "decisions", "--json"]);
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    let payload = json(&output);
    let runbook = payload["files"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["artifact_type"] == "runbook")
        .expect("the parent's runbook classifies in the child");
    assert_eq!(runbook["status"], "valid");
    assert_eq!(runbook["provenance"]["source"], PARENT_SOURCE);
    assert!(payload.get("artifact_spec_bundle").is_none());
    assert_eq!(payload["artifact_spec_bundles"][0]["source"], PARENT_SOURCE);
    assert_eq!(payload["artifact_spec_bundles"][0]["layer"], "inherited");

    // The root of a version-1 child is its corpus: `--corpus .` composes it
    // and lists the inherited type, and a root-level artifact of that type
    // validates, rather than the walk climbing above the repository root.
    let listing = json(&repo.run(&["schema", "--list", "--json", "--corpus", "."]));
    assert_eq!(
        listing["schemas"].as_array().unwrap().last().unwrap(),
        "runbook",
        "{listing}"
    );
    let listing = json(&repo.run(&["templates", "--json", "--corpus", "decisions"]));
    assert_eq!(
        listing["templates"].as_array().unwrap().last().unwrap(),
        "runbook"
    );
    repo.write(
        "root-runbook.md",
        "---\nschema_version: 1\nid: APP-000000000009\ntype: runbook\n---\n# Root Runbook\n\n## Status\n\nActive\n\n## Purpose\n\nRoll a build.\n\n## Steps\n\n1. Deploy.\n",
    );
    let root_file = repo.run(&["validate", "root-runbook.md", "--json"]);
    assert_eq!(root_file.status.code(), Some(0), "{}", stdout(&root_file));
    assert_eq!(json(&root_file)["valid"], true);

    // inspect and improve compose the version-1 closure too.
    repo.write(
        "decisions/runbooks/local.md",
        "---\nschema_version: 1\nid: APP-000000000008\ntype: runbook\n---\n# Local Runbook\n\n## Status\n\nActive\n\n## Purpose\n\nRoll a build.\n\n## Steps\n\n1. Deploy.\n",
    );
    for args in [
        ["inspect", "decisions/runbooks/local.md"].as_slice(),
        ["improve", "decisions/runbooks/local.md"].as_slice(),
    ] {
        let output = repo.run(args);
        assert!(
            stdout(&output).starts_with("Artifact Type: Runbook"),
            "{args:?}: {}",
            stdout(&output)
        );
    }
    let inspected = repo.run(&["inspect", "decisions"]);
    assert!(
        stdout(&inspected).contains("Runbooks: 1"),
        "{}",
        stdout(&inspected)
    );
    fs::remove_file(repo.root().join("decisions/runbooks/local.md")).unwrap();

    // `new` composes the closure first, so an inherited type scaffolds.
    fs::create_dir_all(repo.root().join("decisions/runbooks")).unwrap();
    let created = repo.run(&["new", "runbook", "decisions/runbooks/rotate-keys.md"]);
    assert_eq!(created.status.code(), Some(0), "{}", stderr(&created));
    let body = fs::read_to_string(repo.root().join("decisions/runbooks/rotate-keys.md")).unwrap();
    assert!(body.contains("type: runbook"), "{body}");
    assert!(body.contains("## Purpose"), "{body}");
    let validated = repo.run(&["validate", "decisions/runbooks/rotate-keys.md", "--json"]);
    assert_eq!(validated.status.code(), Some(0), "{}", stderr(&validated));
    let payload = json(&validated);
    assert_eq!(payload["valid"], true, "{payload}");
    assert_eq!(payload["errors"], serde_json::json!([]));
    // Without the inherited type the same file would be an unknown document;
    // the classification is proven by the corpus-level row.
    let corpus = json(&repo.run(&["validate", "decisions", "--json"]));
    let scaffolded = corpus["files"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["path"] == "runbooks/rotate-keys.md")
        .expect("the scaffolded runbook is a corpus row");
    assert_eq!(scaffolded["artifact_type"], "runbook");
    assert_eq!(scaffolded["provenance"]["source"], CHILD_SOURCE);
}

fn git(root: &Path, args: &[&str]) {
    let status = Command::new("git")
        .args(["-c", "user.name=t", "-c", "user.email=t@example.invalid"])
        .args(args)
        .current_dir(root)
        .status()
        .unwrap();
    assert!(status.success(), "git {args:?}");
}

fn exported_types(output: &Output) -> Vec<String> {
    assert_eq!(output.status.code(), Some(0), "{}", stderr(output));
    let mut types: Vec<String> = json(output)["artifacts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a["type"].as_str().unwrap().to_string())
        .collect();
    types.sort();
    types.dedup();
    types
}

#[test]
fn historical_exports_classify_under_the_bundles_pinned_at_that_revision() {
    let root = fixture_copy("export-at");
    git(&root, &["init", "-q"]);
    git(&root, &["add", "-A"]);
    git(&root, &["commit", "-qm", "one"]);

    // Commit two: the parent drops `runbook` and the child re-pins the parent.
    let parent_bundle: serde_json::Value = serde_json::from_slice(
        &fs::read(root.join("vendor/standards/.decided/artifact-specs.json")).unwrap(),
    )
    .unwrap();
    let kept: Vec<String> = parent_bundle["artifact_specs"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| e["name"] != "runbook")
        .map(|e| e.to_string())
        .collect();
    let kept: Vec<&str> = kept.iter().map(String::as_str).collect();
    write_and_repin(
        &root,
        "vendor/standards/.decided/artifact-specs.json",
        "vendor/standards/.decided/config.yaml",
        &kept,
    );
    let digest = run_in(
        &root,
        &[
            "corpus",
            "digest",
            "--root",
            "vendor/standards",
            "--corpus",
            "decisions",
            "--version",
            "2",
        ],
    );
    let digest = stdout(&digest).trim().to_string();
    let manifest = root.join(".decided/corpus.md");
    let text = fs::read_to_string(&manifest).unwrap();
    let start = text.find("digest: sha256-v2:").unwrap();
    let end = start + "digest: sha256-v2:".len() + 64;
    fs::write(
        &manifest,
        format!("{}digest: {digest}{}", &text[..start], &text[end..]),
    )
    .unwrap();
    git(&root, &["commit", "-qam", "two"]);

    let then = exported_types(&run_in(
        &root,
        &["export", "decisions", "--at", "HEAD~1", "--json"],
    ));
    assert_eq!(then, ["decision", "policy", "runbook"]);
    let now = exported_types(&run_in(
        &root,
        &["export", "decisions", "--at", "HEAD", "--json"],
    ));
    assert_eq!(now, ["decision", "policy"]);

    // History does not depend on the working tree's pin.
    let mut bytes = fs::read(root.join(".decided/artifact-specs.json")).unwrap();
    bytes.extend_from_slice(b" \n");
    fs::write(root.join(".decided/artifact-specs.json"), bytes).unwrap();
    let then = exported_types(&run_in(
        &root,
        &["export", "decisions", "--at", "HEAD~1", "--json"],
    ));
    assert_eq!(then, ["decision", "policy", "runbook"]);
}
