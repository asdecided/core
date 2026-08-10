//! Black-box contract tests for the first corpus-federation increment.
//!
//! These tests intentionally exercise only the public `decided` binary. They
//! are expected to fail until the corresponding stacked engine slices land;
//! do not weaken or ignore them to make an intermediate branch green.

mod federation_support;

use std::fs;

use federation_support::{
    assert_exit, assert_inherited_provenance, assert_local_provenance, assert_success, combined,
    override_yaml, parse_json, parse_json_lines, qualified, stdout, FederationRepo,
    CHILD_DECISION_ID, CHILD_REQUIREMENT_ID, CHILD_SOURCE, CONSTRAINT_RULE_ID, FORBIDDEN_MARKER,
    PARENT_ALIAS, PARENT_DECISION_ID, PARENT_REQUIREMENT_ID, PARENT_SOURCE,
};
use serde_json::Value;

fn assert_stable_failure(output: &std::process::Output, code: &str, context: &str) {
    assert_exit(output, 1, context);
    let rendered = combined(output);
    assert!(
        rendered.contains(code),
        "{context} omitted stable code {code:?}:\n{rendered}"
    );
}

fn record_with_id<'a>(records: &'a [Value], id: &str, context: &str) -> &'a Value {
    records
        .iter()
        .find(|record| record["id"].as_str() == Some(id))
        .unwrap_or_else(|| panic!("{context} omitted {id}: {records:?}"))
}

fn assert_exact_provenance(
    provenance: &Value,
    source: &str,
    layer: &str,
    pin: Option<&str>,
    context: &str,
) {
    assert_eq!(provenance["source"].as_str(), Some(source), "{context}");
    assert_eq!(provenance["layer"].as_str(), Some(layer), "{context}");
    match pin {
        Some(pin) => assert_eq!(provenance["pin"].as_str(), Some(pin), "{context}"),
        None => assert!(provenance.get("pin").is_none(), "{context}: {provenance}"),
    }
}

#[test]
fn vendored_parent_is_verified_and_shared_by_read_surfaces() {
    let repo = FederationRepo::new("vendored-happy");
    repo.write_child_requirement_reference(&qualified(PARENT_DECISION_ID));
    let digest = repo.activate();

    let validation = repo.run(&["validate", "decisions", "--json"]);
    assert_success(&validation, "validate one vendored parent");

    let relationships = repo.run(&["relationships", "decisions", "--validate", "--json"]);
    assert_success(
        &relationships,
        "resolve a child relationship through the composed corpus",
    );

    let qualified_id = qualified(PARENT_DECISION_ID);
    let resolution = repo.run(&["resolve", &qualified_id, "decisions", "--json"]);
    assert_success(&resolution, "resolve a qualified inherited decision");
    let resolved = parse_json(&resolution, "qualified inherited resolution");
    assert_eq!(resolved["id"].as_str(), Some(PARENT_DECISION_ID));
    assert_inherited_provenance(&resolved, &digest);

    let search = repo.run(&[
        "find",
        "Parent Guardrail",
        "decisions",
        "--no-cache",
        "--json",
    ]);
    assert_success(&search, "search the composed corpus");
    let searched = parse_json(&search, "federated search");
    assert!(
        stdout(&search).contains(PARENT_DECISION_ID),
        "search omitted inherited decision: {searched}"
    );
    assert_inherited_provenance(&searched, &digest);
}

#[test]
fn digest_mismatch_blocks_overlay_with_a_stable_code() {
    let repo = FederationRepo::new("digest-mismatch");
    repo.activate();
    repo.append(
        "vendor/standards/decisions/decisions/parent-guardrail.md",
        "\nA post-pin byte change.\n",
    );

    let output = repo.run(&["validate", "decisions", "--json"]);
    assert_stable_failure(
        &output,
        "parent-corpus-digest-mismatch",
        "a stale parent digest must block composition",
    );
    assert!(
        !combined(&output).contains("Parent Guardrail"),
        "a failed pin must not expose inherited artifact content"
    );
}

#[test]
fn source_mismatch_blocks_overlay_with_a_stable_code() {
    let repo = FederationRepo::new("source-mismatch");
    let digest = repo.parent_digest();
    repo.write_manifest(
        "acme/not-the-parent",
        "vendor/standards",
        "decisions",
        &digest,
        None,
    );

    let output = repo.run(&["validate", "decisions", "--json"]);
    assert_stable_failure(
        &output,
        "parent-corpus-source-mismatch",
        "a manifest/config source mismatch must block composition",
    );
    let rendered = combined(&output);
    assert!(rendered.contains(PARENT_SOURCE));
    assert!(rendered.contains("acme/not-the-parent"));
}

#[test]
fn transitive_parent_is_rejected_before_overlay() {
    let repo = FederationRepo::new("transitive-parent");
    repo.activate();
    repo.write_parent_manifest();

    let output = repo.run(&["resolve", PARENT_DECISION_ID, "decisions", "--json"]);
    assert_stable_failure(
        &output,
        "parent-corpus-transitive-inheritance",
        "a parent-of-parent declaration must fail before resolution",
    );
}

#[test]
fn absolute_and_parent_component_roots_are_rejected() {
    let absolute = FederationRepo::new("absolute-root");
    let absolute_root = absolute.parent_root().to_string_lossy().into_owned();
    absolute.write_manifest(
        PARENT_SOURCE,
        &absolute_root,
        "decisions",
        "sha256:0000000000000000000000000000000000000000000000000000000000000000",
        None,
    );
    let output = absolute.run(&["validate", "decisions", "--json"]);
    assert_stable_failure(
        &output,
        "parent-corpus-path-escape",
        "an absolute materialisation root must be rejected",
    );

    let traversal = FederationRepo::new("parent-component");
    traversal.write_manifest(
        PARENT_SOURCE,
        "vendor/../vendor/standards",
        "decisions",
        "sha256:0000000000000000000000000000000000000000000000000000000000000000",
        None,
    );
    let output = traversal.run(&["validate", "decisions", "--json"]);
    assert_stable_failure(
        &output,
        "parent-corpus-path-escape",
        "a lexical parent component must be rejected even if it resolves inside the child",
    );
}

#[cfg(unix)]
#[test]
fn symlinked_parent_root_is_rejected() {
    use std::os::unix::fs::symlink;

    let repo = FederationRepo::new("symlink-root");
    let digest = repo.parent_digest();
    symlink("standards", repo.root().join("vendor/standards-link"))
        .expect("create parent-root symlink fixture");
    repo.write_manifest(
        PARENT_SOURCE,
        "vendor/standards-link",
        "decisions",
        &digest,
        None,
    );

    let output = repo.run(&["validate", "decisions", "--json"]);
    assert_stable_failure(
        &output,
        "parent-corpus-symlink-traversal",
        "a symlink in the materialisation path must be rejected",
    );
}

#[test]
fn canonical_id_collision_has_no_implicit_precedence() {
    let repo = FederationRepo::new("canonical-collision");
    repo.write_child_collision();
    repo.activate();

    let validation = repo.run(&["validate", "decisions", "--json"]);
    assert_stable_failure(
        &validation,
        "cross-corpus-canonical-id-collision",
        "a child/parent canonical-id collision must fail deterministically",
    );

    let resolution = repo.run(&["resolve", PARENT_DECISION_ID, "decisions", "--json"]);
    assert_exit(
        &resolution,
        1,
        "unqualified collision resolution must not choose either layer",
    );
    assert!(
        combined(&resolution).contains("ambiguous") || combined(&resolution).contains("collision"),
        "collision resolution needs an explicit remedy: {}",
        combined(&resolution)
    );
}

#[test]
fn qualified_lookup_is_source_bounded_and_requires_a_canonical_id() {
    let repo = FederationRepo::new("qualified-lookup");
    let digest = repo.activate();

    let unqualified = repo.run(&["resolve", PARENT_DECISION_ID, "decisions", "--json"]);
    assert_success(
        &unqualified,
        "a globally unique inherited canonical id resolves unqualified",
    );
    assert_inherited_provenance(
        &parse_json(&unqualified, "unqualified inherited resolution"),
        &digest,
    );

    let qualified_id = qualified(PARENT_DECISION_ID);
    let qualified_output = repo.run(&["resolve", &qualified_id, "decisions", "--json"]);
    assert_success(&qualified_output, "qualified canonical lookup");
    assert_inherited_provenance(
        &parse_json(&qualified_output, "qualified canonical lookup"),
        &digest,
    );

    let alias = format!("{PARENT_ALIAS}::parent-guardrail");
    let alias_output = repo.run(&["resolve", &alias, "decisions", "--json"]);
    assert_exit(
        &alias_output,
        1,
        "an alias after the qualifier must be rejected",
    );
    assert!(
        combined(&alias_output).contains("canonical"),
        "qualified alias rejection must explain the canonical-id requirement: {}",
        combined(&alias_output)
    );
}

#[test]
fn valid_override_selects_local_replacement_and_preserves_parent() {
    let repo = FederationRepo::new("valid-override");
    repo.write_child_replacement();
    let mapping = override_yaml(
        &qualified(PARENT_REQUIREMENT_ID),
        CHILD_REQUIREMENT_ID,
        CHILD_DECISION_ID,
    );
    let digest = repo.activate_with_override(Some(&mapping));

    let validation = repo.run(&["validate", "decisions", "--json"]);
    assert_success(&validation, "validate an explicit Decision-backed override");

    let effective = repo.run(&["resolve", PARENT_REQUIREMENT_ID, "decisions", "--json"]);
    assert_success(&effective, "resolve the effective unqualified replacement");
    let effective_json = parse_json(&effective, "effective override resolution");
    assert_eq!(effective_json["id"].as_str(), Some(CHILD_REQUIREMENT_ID));
    assert_local_provenance(&effective_json);

    let original_id = qualified(PARENT_REQUIREMENT_ID);
    let original = repo.run(&["resolve", &original_id, "decisions", "--json"]);
    assert_success(
        &original,
        "keep the overridden parent qualified-addressable",
    );
    let original_json = parse_json(&original, "overridden parent resolution");
    assert_eq!(original_json["id"].as_str(), Some(PARENT_REQUIREMENT_ID));
    assert_inherited_provenance(&original_json, &digest);

    let exported = repo.run(&["export", "decisions", "--documents"]);
    assert_success(&exported, "export both sides of an override");
    let rendered = stdout(&exported);
    for expected in [
        PARENT_REQUIREMENT_ID,
        CHILD_REQUIREMENT_ID,
        CHILD_DECISION_ID,
        "overridden",
    ] {
        assert!(
            rendered.contains(expected),
            "export omitted {expected}: {rendered}"
        );
    }
    let documents = parse_json_lines(&exported, "override documents export");
    let parent = record_with_id(
        &documents,
        PARENT_REQUIREMENT_ID,
        "override documents export",
    );
    let replacement = record_with_id(
        &documents,
        CHILD_REQUIREMENT_ID,
        "override documents export",
    );
    let parent_provenance = &parent["metadata"]["provenance"];
    let replacement_provenance = &replacement["metadata"]["provenance"];
    assert_exact_provenance(
        parent_provenance,
        PARENT_SOURCE,
        "inherited",
        Some(&digest),
        "overridden parent document provenance",
    );
    assert_exact_provenance(
        replacement_provenance,
        CHILD_SOURCE,
        "local",
        None,
        "replacement document provenance",
    );
    for (provenance, state) in [
        (parent_provenance, "overridden"),
        (replacement_provenance, "replacement"),
    ] {
        let mapping = &provenance["overrides"][0];
        assert_eq!(mapping["state"].as_str(), Some(state));
        assert_eq!(
            mapping["parent"],
            serde_json::json!({"source": PARENT_SOURCE, "id": PARENT_REQUIREMENT_ID})
        );
        assert_eq!(
            mapping["replacement"],
            serde_json::json!({"source": CHILD_SOURCE, "id": CHILD_REQUIREMENT_ID})
        );
        assert_eq!(
            mapping["rationale"],
            serde_json::json!({"source": CHILD_SOURCE, "id": CHILD_DECISION_ID})
        );
    }
}

#[test]
fn invalid_overrides_fail_closed_with_one_stable_code() {
    struct Case {
        label: &'static str,
        parent: String,
        replacement: String,
        rationale: String,
        mutate: Option<fn(&FederationRepo)>,
        semantic: &'static str,
    }

    fn reject_rationale(repo: &FederationRepo) {
        repo.write_child_rationale("Rejected");
    }

    let cases = [
        Case {
            label: "override-parent-missing",
            parent: qualified("STD-KZKMJAF599TB"),
            replacement: CHILD_REQUIREMENT_ID.to_string(),
            rationale: CHILD_DECISION_ID.to_string(),
            mutate: None,
            semantic: "parent",
        },
        Case {
            label: "override-replacement-missing",
            parent: qualified(PARENT_REQUIREMENT_ID),
            replacement: "APP-KZKMJA3YK5Y1".to_string(),
            rationale: CHILD_DECISION_ID.to_string(),
            mutate: None,
            semantic: "replacement",
        },
        Case {
            label: "override-type-mismatch",
            parent: qualified(PARENT_REQUIREMENT_ID),
            replacement: CHILD_DECISION_ID.to_string(),
            rationale: CHILD_DECISION_ID.to_string(),
            mutate: None,
            semantic: "type",
        },
        Case {
            label: "override-rationale-retired",
            parent: qualified(PARENT_REQUIREMENT_ID),
            replacement: CHILD_REQUIREMENT_ID.to_string(),
            rationale: CHILD_DECISION_ID.to_string(),
            mutate: Some(reject_rationale),
            semantic: "rationale",
        },
        Case {
            label: "override-rationale-parent",
            parent: qualified(PARENT_REQUIREMENT_ID),
            replacement: CHILD_REQUIREMENT_ID.to_string(),
            rationale: qualified(PARENT_DECISION_ID),
            mutate: None,
            semantic: "local",
        },
    ];

    for case in cases {
        let repo = FederationRepo::new(case.label);
        repo.write_child_replacement();
        if let Some(mutate) = case.mutate {
            mutate(&repo);
        }
        let mapping = override_yaml(&case.parent, &case.replacement, &case.rationale);
        repo.activate_with_override(Some(&mapping));
        let output = repo.run(&["validate", "decisions", "--json"]);
        assert_stable_failure(&output, "cross-corpus-invalid-override", case.label);
        let rendered = combined(&output).to_lowercase();
        assert!(
            rendered.contains(case.semantic),
            "{} omitted semantic context {:?}: {}",
            case.label,
            case.semantic,
            rendered
        );
    }
}

#[test]
fn inherited_scope_and_constraints_govern_child_code() {
    let repo = FederationRepo::new("inherited-enforcement");
    let digest = repo.activate();

    let scope = repo.run(&["decisions-for", "src/guarded.rs", "decisions", "--json"]);
    assert_success(&scope, "route an inherited Decision to a child path");
    let scope_json = parse_json(&scope, "inherited decisions-for result");
    assert!(stdout(&scope).contains(PARENT_DECISION_ID));
    assert_inherited_provenance(&scope_json, &digest);

    repo.write(
        "src/guarded.rs",
        &format!("pub const MARKER: &str = \"{FORBIDDEN_MARKER}\";\n"),
    );
    let sentry = repo.run(&[
        "sentry",
        "decisions",
        "--repository",
        ".",
        "--full",
        "--json",
    ]);
    assert_exit(&sentry, 1, "inherited Sentry rule must block child code");
    let sentry_text = stdout(&sentry);
    for expected in [PARENT_DECISION_ID, CONSTRAINT_RULE_ID, "src/guarded.rs"] {
        assert!(
            sentry_text.contains(expected),
            "Sentry omitted {expected}: {sentry_text}"
        );
    }

    let gate = repo.run(&[
        "gate",
        "decisions",
        "--code",
        "--repository",
        ".",
        "--full",
        "--json",
    ]);
    assert_exit(&gate, 1, "gate --code must compose inherited Sentry rules");
    let gate_text = stdout(&gate);
    assert!(gate_text.contains(PARENT_DECISION_ID));
    assert!(gate_text.contains(CONSTRAINT_RULE_ID));

    for (name, args) in [
        (
            "sentry",
            vec![
                "sentry",
                "decisions",
                "--repository",
                ".",
                "--full",
                "--local-only",
            ],
        ),
        (
            "gate",
            vec![
                "gate",
                "decisions",
                "--code",
                "--repository",
                ".",
                "--full",
                "--local-only",
            ],
        ),
    ] {
        let bypass = repo.run(&args);
        assert_exit(
            &bypass,
            2,
            &format!("{name} must not expose a local-only enforcement bypass"),
        );
    }
}

#[test]
fn exports_include_parent_by_default_and_local_only_excludes_it() {
    let repo = FederationRepo::new("export-composition");
    let digest = repo.activate();

    for mode in [None, Some("--documents"), Some("--graph")] {
        let mut default_args = vec!["export", "decisions"];
        if let Some(mode) = mode {
            default_args.push(mode);
        }
        let default = repo.run(&default_args);
        assert_success(&default, "default federated export");
        let default_text = stdout(&default);
        assert!(default_text.contains(PARENT_DECISION_ID));
        assert!(default_text.contains(CHILD_DECISION_ID));
        assert!(default_text.contains(PARENT_SOURCE));
        assert!(default_text.contains(&digest));
        match mode {
            None => {
                let payload = parse_json(&default, "default viewer export");
                let artifacts = payload["artifacts"].as_array().unwrap();
                assert_exact_provenance(
                    &record_with_id(artifacts, PARENT_DECISION_ID, "viewer artifacts")
                        ["provenance"],
                    PARENT_SOURCE,
                    "inherited",
                    Some(&digest),
                    "inherited viewer artifact provenance",
                );
                assert_exact_provenance(
                    &record_with_id(artifacts, CHILD_DECISION_ID, "viewer artifacts")["provenance"],
                    CHILD_SOURCE,
                    "local",
                    None,
                    "local viewer artifact provenance",
                );
            }
            Some("--documents") => {
                let documents = parse_json_lines(&default, "default documents export");
                let parent = record_with_id(&documents, PARENT_DECISION_ID, "documents export");
                assert_eq!(parent["metadata"]["source"].as_str(), Some(PARENT_SOURCE));
                assert_exact_provenance(
                    &parent["metadata"]["provenance"],
                    PARENT_SOURCE,
                    "inherited",
                    Some(&digest),
                    "inherited document provenance",
                );
            }
            Some("--graph") => {
                let payload = parse_json(&default, "default graph export");
                let nodes = payload["nodes"].as_array().unwrap();
                assert_exact_provenance(
                    &record_with_id(nodes, PARENT_DECISION_ID, "graph nodes")["provenance"],
                    PARENT_SOURCE,
                    "inherited",
                    Some(&digest),
                    "inherited graph node provenance",
                );
                let edge = payload["edges"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|edge| {
                        edge["source"].as_str() == Some(PARENT_REQUIREMENT_ID)
                            && edge["target"].as_str() == Some(PARENT_DECISION_ID)
                    })
                    .expect("graph retained inherited parent edge");
                assert_eq!(
                    edge["source_identity"],
                    serde_json::json!({"source": PARENT_SOURCE, "id": PARENT_REQUIREMENT_ID})
                );
                assert_eq!(
                    edge["target_identity"],
                    serde_json::json!({"source": PARENT_SOURCE, "id": PARENT_DECISION_ID})
                );
                assert_exact_provenance(
                    &edge["provenance"],
                    PARENT_SOURCE,
                    "inherited",
                    Some(&digest),
                    "inherited graph edge provenance",
                );
            }
            Some(other) => panic!("unexpected export mode {other}"),
        }

        let mut local_args = vec!["export", "decisions", "--local-only"];
        if let Some(mode) = mode {
            local_args.push(mode);
        }
        let local = repo.run(&local_args);
        assert_success(&local, "explicit local-only export");
        let local_text = stdout(&local);
        assert!(local_text.contains(CHILD_DECISION_ID));
        assert!(local_text.contains(CHILD_SOURCE));
        assert!(
            !local_text.contains(PARENT_DECISION_ID),
            "local-only {mode:?} export leaked an inherited record: {local_text}"
        );
        assert!(
            !local_text.contains(PARENT_SOURCE),
            "local-only {mode:?} export leaked inherited source provenance"
        );
    }

    let okf = repo.run(&["export", "decisions", "--okf", "--out", "local-okf"]);
    assert_success(&okf, "local-only OKF export from the child corpus root");
    let okf_index =
        fs::read_to_string(repo.root().join("local-okf/index.md")).expect("read local OKF index");
    assert!(okf_index.contains("Local Rationale"));
    assert!(!okf_index.contains("Parent Guardrail"));
}

#[test]
fn no_manifest_keeps_released_resolve_bytes_and_deterministic_exports() {
    let repo = FederationRepo::new("no-manifest-parity");
    assert!(!repo.root().join(".decided/corpus.md").exists());

    let child_validation = repo.run(&["validate", "decisions", "--json"]);
    assert_success(
        &child_validation,
        "the deterministic child fixture validates independently",
    );
    let parent_validation = repo.run(&["validate", "vendor/standards/decisions", "--json"]);
    assert_success(
        &parent_validation,
        "the deterministic parent fixture validates independently",
    );

    let json = repo.run(&["resolve", CHILD_DECISION_ID, "decisions", "--json"]);
    assert_success(&json, "no-manifest JSON resolve");
    assert_eq!(
        stdout(&json),
        format!(
            "{{\n  \"schema_version\": \"1\",\n  \"id\": \"{CHILD_DECISION_ID}\",\n  \"type\": \"decision\",\n  \"title\": \"Local Rationale\",\n  \"path\": \"decisions/decisions/local-rationale.md\"\n}}\n"
        )
    );

    let human = repo.run(&["resolve", CHILD_DECISION_ID, "decisions"]);
    assert_success(&human, "no-manifest human resolve");
    assert_eq!(
        stdout(&human),
        format!(
            "{CHILD_DECISION_ID}\n\nType: decision\nTitle: Local Rationale\nPath: decisions/decisions/local-rationale.md\n"
        )
    );

    let first = repo.run(&["export", "decisions", "--documents"]);
    let second = repo.run(&["export", "decisions", "--documents"]);
    assert_success(&first, "first no-manifest export");
    assert_success(&second, "second no-manifest export");
    assert_eq!(
        first.stdout, second.stdout,
        "no-manifest export changed across identical runs"
    );
    assert_eq!(
        first.stderr, second.stderr,
        "no-manifest diagnostics changed across identical runs"
    );
}

#[test]
fn mutation_commands_never_change_the_materialised_parent_tree() {
    let repo = FederationRepo::new("parent-immutability");
    repo.activate();
    repo.write(
        "decisions/requirements/local-legacy.md",
        "# Requirement: Local Legacy\n\n## Problem\n\nA local legacy artifact needs migration.\n\n## Requirements\n\n- [REQ-001] The migration MUST stay local.\n\n## Acceptance Criteria\n\n- Only child bytes change.\n",
    );
    let parent_before = repo.parent_snapshot();
    fs::create_dir_all(repo.root().join("vendor/sibling"))
        .expect("create traversal sibling fixture");

    let commands: [(&str, Vec<&str>, i32); 7] = [
        (
            "init guidance",
            vec!["init", "--parent-corpus", "--json"],
            0,
        ),
        (
            "new local artifact",
            vec![
                "new",
                "requirement",
                "decisions/requirements/generated-local.md",
                "--json",
            ],
            0,
        ),
        (
            "migrate local metadata",
            vec!["migrate", "metadata", "decisions", "--json"],
            0,
        ),
        (
            "rename local artifact",
            vec![
                "rename",
                CHILD_REQUIREMENT_ID,
                "APP-RENAMED",
                "decisions",
                "--apply",
                "--json",
            ],
            0,
        ),
        (
            "generate local agent rules",
            vec!["export", "decisions", "--agent-rules", "--client", "agents"],
            0,
        ),
        (
            "install local skill",
            vec![
                "skill",
                "install",
                "decided-artifacts",
                "--dir",
                ".",
                "--json",
            ],
            0,
        ),
        (
            "install local hook",
            vec!["hook", "install", "--dir", ".", "--json"],
            0,
        ),
    ];

    for (name, args, expected_exit) in commands {
        let output = repo.run(&args);
        assert_exit(&output, expected_exit, name);
        assert_eq!(
            repo.parent_snapshot(),
            parent_before,
            "{name} changed materialised parent bytes"
        );
    }

    for (name, args) in [
        (
            "scaffold beneath inherited root",
            vec![
                "new",
                "requirement",
                "vendor/standards/decisions/requirements/forbidden-local-write.md",
                "--json",
            ],
        ),
        (
            "migrate inherited corpus",
            vec![
                "migrate",
                "metadata",
                "vendor/standards/decisions",
                "--json",
            ],
        ),
        (
            "generate agent rules beneath inherited root",
            vec![
                "export",
                "decisions",
                "--agent-rules",
                "--out",
                "vendor/standards",
            ],
        ),
        (
            "write HTML export beneath inherited root",
            vec![
                "export",
                "decisions",
                "--html",
                "--out",
                "vendor/standards/forbidden.html",
            ],
        ),
        (
            "write OKF export beneath inherited root",
            vec![
                "export",
                "decisions",
                "--okf",
                "--out",
                "vendor/standards/forbidden-okf",
            ],
        ),
        (
            "write HTML through a sibling traversal beneath inherited root",
            vec![
                "export",
                "decisions",
                "--html",
                "--out",
                "vendor/sibling/../standards/traversal-forbidden.html",
            ],
        ),
        (
            "write OKF through a sibling traversal beneath inherited root",
            vec![
                "export",
                "decisions",
                "--okf",
                "--out",
                "vendor/sibling/../standards/traversal-forbidden-okf",
            ],
        ),
    ] {
        let output = repo.run(&args);
        assert!(
            !output.status.success(),
            "{name} crossed the inherited write boundary:\n{}",
            combined(&output)
        );
        assert_eq!(
            repo.parent_snapshot(),
            parent_before,
            "{name} changed materialised parent bytes"
        );
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;

        symlink(
            "standards/decisions",
            repo.root().join("vendor/parent-subdir-link"),
        )
        .expect("create parent-subdirectory symlink fixture");
        for (name, args) in [
            (
                "write HTML through a parent-subdirectory symlink and parent component",
                vec![
                    "export",
                    "decisions",
                    "--html",
                    "--out",
                    "vendor/parent-subdir-link/../symlink-forbidden.html",
                ],
            ),
            (
                "write OKF through a parent-subdirectory symlink and parent component",
                vec![
                    "export",
                    "decisions",
                    "--okf",
                    "--out",
                    "vendor/parent-subdir-link/../symlink-forbidden-okf",
                ],
            ),
        ] {
            let output = repo.run(&args);
            assert!(
                !output.status.success(),
                "{name} crossed the inherited write boundary:\n{}",
                combined(&output)
            );
            assert_eq!(
                repo.parent_snapshot(),
                parent_before,
                "{name} changed materialised parent bytes"
            );
        }
    }

    let parent_id = qualified(PARENT_DECISION_ID);
    let inherited_rename = repo.run(&[
        "rename",
        &parent_id,
        "STD-RENAMED",
        "decisions",
        "--apply",
        "--json",
    ]);
    assert_exit(
        &inherited_rename,
        1,
        "rename must refuse an inherited mutation target",
    );
    let refusal = combined(&inherited_rename).to_lowercase();
    assert!(
        refusal.contains("read-only") || refusal.contains("inherited"),
        "rename refusal did not explain the inherited write boundary: {refusal}"
    );
    assert_eq!(repo.parent_snapshot(), parent_before);

    assert!(
        fs::read_dir(repo.parent_root())
            .expect("read final parent tree")
            .next()
            .is_some(),
        "parent fixture unexpectedly disappeared"
    );
}
