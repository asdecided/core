//! The Risk family (ADR-151): the sixth built-in type, registry-driven through
//! the shared generic validator, with one new edge, `related_risks`. The
//! `rust/fixtures/risk-family/` corpus holds a valid Risk linked from every
//! declaring type, the negative boundary cases, and adjacent types carrying a
//! prose `## Risk` heading. These tests pin the family end to end through the
//! CLI, and the golden guard: a corpus without a Risk sees no new keys.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../fixtures/risk-family")
        .canonicalize()
        .expect("risk-family fixture exists")
}

fn scratch(tag: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "asdecided-risk-family-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&root).unwrap();
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

fn json(output: &Output) -> serde_json::Value {
    serde_json::from_str(&stdout(output)).unwrap_or_else(|e| {
        panic!(
            "stdout is not JSON: {e}\n{}\n{}",
            stdout(output),
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

/// `(path suffix, artifact_type, status, issue codes)` for every file.
fn validate_rows(root: &Path) -> Vec<(String, String, String, Vec<String>)> {
    let output = run_in(root, &["validate", ".", "--json"]);
    json(&output)["files"]
        .as_array()
        .expect("files array")
        .iter()
        .map(|file| {
            (
                file["path"].as_str().unwrap().to_string(),
                file["artifact_type"].as_str().unwrap().to_string(),
                file["status"].as_str().unwrap().to_string(),
                file["issues"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|issue| issue["code"].as_str().unwrap().to_string())
                    .collect(),
            )
        })
        .collect()
}

fn row<'a>(
    rows: &'a [(String, String, String, Vec<String>)],
    suffix: &str,
) -> &'a (String, String, String, Vec<String>) {
    rows.iter()
        .find(|(path, ..)| path.ends_with(suffix))
        .unwrap_or_else(|| panic!("no row for {suffix}: {rows:?}"))
}

fn inspect_type(root: &Path, file: &str, body: &str) -> String {
    fs::write(root.join(file), body).unwrap();
    let output = run_in(root, &["inspect", file, "--json"]);
    json(&output)["type"].as_str().unwrap().to_string()
}

#[test]
fn risk_is_a_listed_built_in_with_a_template_that_validates_clean() {
    let root = scratch("template");
    let init = run_in(&root, &["init", "--key", "TST"]);
    assert!(init.status.success(), "{init:?}");

    let listed = json(&run_in(&root, &["schema", "--list", "--json"]));
    let names: Vec<&str> = listed["schemas"]
        .as_array()
        .or_else(|| listed.as_array())
        .expect("schema list")
        .iter()
        .map(|s| s.as_str().or_else(|| s["name"].as_str()).unwrap())
        .collect();
    assert_eq!(
        names,
        [
            "requirement",
            "decision",
            "roadmap",
            "prompt",
            "design",
            "risk"
        ]
    );
    let templates = json(&run_in(&root, &["templates", "--json"]));
    assert_eq!(templates["templates"].as_array().unwrap().len(), 6);
    assert_eq!(templates["templates"][5], "risk");

    let created = run_in(&root, &["new", "risk", "vendor-lock-in.md"]);
    assert!(created.status.success(), "{created:?}");
    let body = fs::read_to_string(root.join("vendor-lock-in.md")).unwrap();
    assert!(body.contains("\ntype: risk\n"), "{body}");
    for heading in [
        "## Risk\n",
        "## Likelihood\n",
        "## Impact\n",
        "## Context\n",
        "## Assumptions\n",
    ] {
        assert!(body.contains(heading), "template lacks {heading}: {body}");
    }
    // Mitigation is optional (ADR-151 as ratified): not scaffolded.
    assert!(!body.contains("## Mitigation"), "{body}");

    let validated = run_in(&root, &["validate", "vendor-lock-in.md"]);
    assert!(validated.status.success(), "{validated:?}");
    let _ = fs::remove_dir_all(root);
}

#[test]
fn a_malformed_risk_still_classifies_as_risk_and_fails_with_type_specific_findings() {
    let rows = validate_rows(&fixture());
    let bad_status = row(&rows, "risks/bad-status.md");
    assert_eq!(bad_status.1, "risk");
    assert_eq!(bad_status.2, "invalid");
    assert_eq!(bad_status.3, ["invalid-risk-status"]);

    // No frontmatter: classified structurally, then failed on the missing
    // required section.
    let missing = row(&rows, "risks/missing-impact.md");
    assert_eq!(missing.1, "risk");
    assert_eq!(missing.2, "invalid");
    assert_eq!(missing.3, ["missing-impact"]);

    let valid = row(&rows, "risks/vendor-lock-in.md");
    assert_eq!((valid.1.as_str(), valid.2.as_str()), ("risk", "valid"));
    // Retired is a legal status value; the retirement shows in relationships.
    let retired = row(&rows, "risks/retired.md");
    assert_eq!((retired.1.as_str(), retired.2.as_str()), ("risk", "valid"));
}

#[test]
fn risk_and_its_section_neighbours_do_not_misclassify_as_each_other() {
    let root = scratch("adjacent");
    // A Decision or Design carrying a prose `## Risk` heading keeps its type.
    assert_eq!(
        inspect_type(
            &root,
            "decision.md",
            "# Use a queue\n\n## Context\n\nC.\n\n## Decision\n\nD.\n\n## Consequences\n\nE.\n\n## Risk\n\nR.\n",
        ),
        "decision"
    );
    assert_eq!(
        inspect_type(
            &root,
            "design.md",
            "# Dashboard\n\n## Context\n\nC.\n\n## User Need\n\nU.\n\n## Design\n\nD.\n\n## Constraints\n\nK.\n\n## Risk\n\nR.\n",
        ),
        "design"
    );
    // A Requirement's or Roadmap's prose `## Risks` is not the Risk family.
    assert_eq!(
        inspect_type(
            &root,
            "requirement.md",
            "# Durable queue\n\n## Problem\n\nP.\n\n## Requirements\n\n- [REQ-001] Keep messages.\n\n## Risks\n\n- Loss.\n",
        ),
        "requirement"
    );
    assert_eq!(
        inspect_type(
            &root,
            "roadmap.md",
            "# Queue work\n\n## Outcomes\n\nO.\n\n## Initiatives\n\nI.\n\n## Risks\n\n- Slip.\n",
        ),
        "roadmap"
    );
    // A Risk carrying `## Context` (shared with Decision and Design) and
    // `## Assumptions` (shared with Requirement and Roadmap) stays a Risk.
    assert_eq!(
        inspect_type(
            &root,
            "risk.md",
            "# Lock-in\n\n## Risk\n\nR.\n\n## Likelihood\n\nL.\n\n## Impact\n\nI.\n\n## Context\n\nC.\n\n## Assumptions\n\nA.\n",
        ),
        "risk"
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn related_risks_resolves_is_status_checked_and_is_unsupported_where_undeclared() {
    let root = fixture();
    let output = run_in(&root, &["relationships", ".", "--validate", "--json"]);
    assert!(!output.status.success(), "the fixture carries two findings");
    let report = json(&output);
    // decision, design, prompt, requirement ×2 → risk; risk → decision.
    assert_eq!(report["relationships_checked"], 6);
    let issues: Vec<(String, String, String)> = report["issues"]
        .as_array()
        .unwrap()
        .iter()
        .map(|issue| {
            (
                issue["source_path"].as_str().unwrap().to_string(),
                issue["relationship"].as_str().unwrap().to_string(),
                issue["code"].as_str().unwrap().to_string(),
            )
        })
        .collect();
    assert_eq!(
        issues,
        [
            (
                "risks/retired.md".to_string(),
                "related_prompts".to_string(),
                "relationship-edge-unsupported".to_string()
            ),
            (
                "requirements/queue.md".to_string(),
                "related_risks".to_string(),
                "relationship-target-superseded".to_string()
            ),
        ]
    );

    let graph = json(&run_in(&root, &["export", ".", "--graph"]));
    let mut sources: Vec<&str> = graph["edges"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|edge| edge["type"] == "related_risks" && edge["target"] == "TST-00000000000R")
        .map(|edge| edge["source"].as_str().unwrap())
        .collect();
    sources.sort_unstable();
    assert_eq!(
        sources,
        [
            "TST-00000000000A",
            "TST-0000000000DG",
            "TST-0000000000PR",
            "TST-0000000000RQ"
        ]
    );
}

#[test]
fn related_risks_is_range_checked_against_the_risk_type() {
    let root = scratch("range");
    fs::write(
        root.join("a.md"),
        "---\nschema_version: 1\nid: TST-00000000000A\ntype: decision\n---\n# A\n\n## Context\n\nC.\n\n## Decision\n\nD.\n\n## Consequences\n\nE.\n\n## Related Risks\n\n- TST-00000000000B\n",
    )
    .unwrap();
    fs::write(
        root.join("b.md"),
        "---\nschema_version: 1\nid: TST-00000000000B\ntype: decision\n---\n# B\n\n## Context\n\nC.\n\n## Decision\n\nD.\n\n## Consequences\n\nE.\n",
    )
    .unwrap();
    let report = json(&run_in(
        &root,
        &["relationships", ".", "--validate", "--json"],
    ));
    assert_eq!(report["issues"][0]["relationship"], "related_risks");
    assert_eq!(
        report["issues"][0]["code"],
        "relationship-target-type-mismatch"
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn stats_reports_risk_artifacts_beside_the_unchanged_requirement_risks_count() {
    let report = json(&run_in(&fixture(), &["stats", ".", "--json"]));
    // `risks` keeps its meaning: Requirement risk lines (ADR-151 decision 6).
    assert_eq!(report["risks"], 1);
    assert_eq!(report["risk_artifacts"]["count"], 4);
    assert_eq!(report["risk_artifacts"]["valid"], 2);
    let invalid: Vec<&str> = report["risk_artifacts"]["invalid"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["file"].as_str().unwrap())
        .collect();
    assert_eq!(invalid, ["risks/bad-status.md", "risks/missing-impact.md"]);
    assert_eq!(report["relationships"]["related_risks"], 4);
    assert!(report.get("risk").is_none(), "{report}");
}

#[test]
fn inspect_counts_risk_only_where_the_directory_holds_one() {
    let root = fixture();
    let inspected = json(&run_in(&root, &["inspect", ".", "--json"]));
    let counts = inspected["summary"]["counts"].as_object().unwrap();
    let keys: Vec<&str> = counts.keys().map(String::as_str).collect();
    assert_eq!(
        keys,
        [
            "requirement",
            "decision",
            "roadmap",
            "prompt",
            "design",
            "risk",
            "unknown"
        ]
    );
    assert_eq!(counts["risk"], 4);
    assert!(stdout(&run_in(&root, &["inspect", "."])).contains("Risks: 4"));
}

#[test]
fn okf_export_writes_type_risk_under_its_own_index_section() {
    let out = scratch("okf");
    let output = run_in(
        &fixture(),
        &["export", ".", "--okf", "--out", out.to_str().unwrap()],
    );
    assert!(output.status.success(), "{output:?}");
    let risk = fs::read_to_string(out.join("risks/vendor-lock-in.md")).unwrap();
    assert!(risk.starts_with("---\ntype: Risk\n"), "{risk}");
    let index = fs::read_to_string(out.join("index.md")).unwrap();
    let designs = index.find("## Designs").expect("designs section");
    let risks = index.find("## Risks").expect("risks section");
    assert!(
        designs < risks,
        "Risk follows the five fixed sections: {index}"
    );
    assert!(
        index.contains("- [Vendor lock-in on the managed queue](risks/vendor-lock-in.md)"),
        "{index}"
    );
    let _ = fs::remove_dir_all(out);
}

#[test]
fn a_corpus_without_a_risk_gains_no_risk_keys_or_sections() {
    let root = scratch("golden");
    fs::write(
        root.join("decision.md"),
        "---\nschema_version: 1\nid: TST-00000000000A\ntype: decision\n---\n# A\n\n## Context\n\nC.\n\n## Decision\n\nD.\n\n## Consequences\n\nE.\n",
    )
    .unwrap();
    let stats = stdout(&run_in(&root, &["stats", ".", "--json"]));
    assert!(!stats.contains("risk_artifacts"), "{stats}");
    assert!(!stats.contains("related_risks"), "{stats}");
    let text = stdout(&run_in(&root, &["stats", "."]));
    assert!(!text.contains("Risks\n====="), "{text}");
    let portfolio = stdout(&run_in(&root, &["portfolio", "."]));
    assert!(!portfolio.contains("Risk"), "{portfolio}");
    let inspected = json(&run_in(&root, &["inspect", ".", "--json"]));
    let counts: Vec<&str> = inspected["summary"]["counts"]
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(
        counts,
        [
            "requirement",
            "decision",
            "roadmap",
            "prompt",
            "design",
            "unknown"
        ]
    );
    let inspected = stdout(&run_in(&root, &["inspect", "."]));
    assert!(!inspected.contains("Risks:"), "{inspected}");

    let out = scratch("golden-okf");
    let output = run_in(
        &root,
        &["export", ".", "--okf", "--out", out.to_str().unwrap()],
    );
    assert!(output.status.success(), "{output:?}");
    let index = fs::read_to_string(out.join("index.md")).unwrap();
    assert!(!index.contains("## Risks"), "{index}");
    let _ = fs::remove_dir_all(root);
    let _ = fs::remove_dir_all(out);
}
