use std::fs;
use std::path::{Path, PathBuf};

use rac_engine::corpus::{ArtifactKey, ArtifactPath, Layer};
use rac_engine::identity::{artifact_identifier, artifact_identifiers};

const DECISION: &str = r#"---
schema_version: 1
id: APP-KWJ4VMKVSS65
type: decision
---
# ADR-001: Keep It Local

## Status

Accepted

## Context

The fixture needs one decision.

## Decision

Keep the released projection exact.

## Consequences

The source-aware fields remain internal.
"#;

const REQUIREMENT: &str = r#"---
schema_version: 1
id: APP-KWJ8S53D06CH
type: requirement
---
# Requirement: Preserve Identity

## Status

Accepted

## Problem

Endpoints need stable source-aware paths.

## Requirements

- [REQ-001] The endpoint MUST retain its corpus source.

## Related Decisions

- APP-KWJ4VMKVSS65
"#;

fn scratch(tag: &str) -> PathBuf {
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root =
        std::env::temp_dir().join(format!("asdecided-{tag}-{}-{unique}", std::process::id()));
    fs::create_dir_all(root.join(".decided")).unwrap();
    fs::create_dir_all(root.join("decisions")).unwrap();
    fs::write(
        root.join(".decided/config.yaml"),
        "repository_key: APP\ncorpus:\n  source: acme/app\n",
    )
    .unwrap();
    fs::write(root.join("decisions/adr-001.md"), DECISION).unwrap();
    root
}

fn corpus_arg(root: &Path) -> String {
    root.join("decisions").to_string_lossy().into_owned()
}

#[test]
fn stable_identity_survives_equivalent_clone_roots() {
    let left_root = scratch("clone-a");
    let right_root = scratch("clone-b");
    let left = rac_engine::relationships::corpus_items(&corpus_arg(&left_root), true);
    let right = rac_engine::relationships::corpus_items(&corpus_arg(&right_root), true);

    assert_eq!(left.len(), 1);
    assert_eq!(right.len(), 1);
    assert_eq!(left[0].origin, right[0].origin);
    assert_eq!(
        left[0].key,
        ArtifactKey::new("acme/app", "APP-KWJ4VMKVSS65")
    );
    assert_eq!(left[0].key, right[0].key);
    assert_eq!(
        left[0].artifact_path,
        ArtifactPath::new("acme/app", "adr-001.md")
    );
    assert_eq!(left[0].artifact_path, right[0].artifact_path);
    assert_eq!(left[0].origin.layer, Layer::Local);
    assert_ne!(left[0].locator.path, right[0].locator.path);

    let _ = fs::remove_dir_all(left_root);
    let _ = fs::remove_dir_all(right_root);
}

#[test]
fn no_manifest_keeps_the_released_index_projection_byte_exact() {
    let root = scratch("parity");
    assert!(!root.join(".decided/corpus.md").exists());
    let directory = corpus_arg(&root);
    let items = rac_engine::relationships::corpus_items(&directory, true);

    // This is the complete path-only projection used before the source-aware
    // substrate. It intentionally does not inspect the new identity fields.
    let legacy = rac_engine::index::RepositoryIndex {
        directory: directory.clone(),
        recursive: true,
        artifacts: items
            .iter()
            .map(|item| rac_engine::index::IndexEntry {
                id: artifact_identifier(&item.artifact, item.spec, &item.path),
                artifact_type: rac_engine::classify::classify(&item.artifact).artifact_type,
                title: item.artifact.product.title.clone(),
                path: item.path.clone(),
                aliases: artifact_identifiers(&item.artifact, item.spec, &item.path),
            })
            .collect(),
    };
    let source_aware = rac_engine::index::build_repository_index(&directory, true);

    assert_eq!(
        rac_engine::output::render_index_json(&source_aware),
        rac_engine::output::render_index_json(&legacy)
    );
    assert_eq!(
        rac_engine::output::render_index_human(&source_aware),
        rac_engine::output::render_index_human(&legacy)
    );

    let derived = rac_engine::derived::build_derived_index(&directory, true);
    assert_eq!(derived.layers.len(), 1);
    assert_eq!(derived.layers[0].source, "acme/app");
    assert_eq!(derived.source_artifacts.len(), derived.index_entries.len());
    assert_eq!(derived.source_artifacts[0].key, items[0].key);
    assert_eq!(derived.source_artifacts[0].path, items[0].artifact_path);
    assert_eq!(derived.index_entries[0].key, Some(items[0].key.clone()));
    assert_eq!(
        derived.index_entries[0].artifact_path,
        Some(items[0].artifact_path.clone())
    );
    let resolved =
        rac_engine::resolve::resolve_in_index(&derived.index_entries, "APP-KWJ4VMKVSS65")
            .artifact
            .expect("source-aware resolved artifact");
    assert_eq!(resolved.key, Some(items[0].key.clone()));
    assert_eq!(resolved.origin, Some(items[0].origin.clone()));

    // The frozen v1 codec remains untouched in this substrate-only change.
    assert_eq!(rac_engine::index_store::STORE_LAYOUT_VERSION, "v1");

    let _ = fs::remove_dir_all(root);
}

#[test]
fn validation_and_relationships_retain_source_aware_endpoints() {
    let root = scratch("relationships");
    fs::write(root.join("decisions/req-002.md"), REQUIREMENT).unwrap();
    let directory = corpus_arg(&root);
    let items = rac_engine::relationships::corpus_items(&directory, true);
    let rows = rac_engine::relationships::rows_from_corpus_items(&items);

    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].key.source, "acme/app");
    assert_eq!(rows[1].artifact_path.source, "acme/app");
    assert!(rows.iter().all(|row| row.origin.layer == Layer::Local));

    let relationships = rac_engine::relationships::relationships_from_corpus(&items);
    let edge = relationships
        .iter()
        .find(|edge| edge.relationship == "related_decisions")
        .expect("related decision edge");
    assert_eq!(
        edge.source_artifact,
        Some(ArtifactPath::new("acme/app", "req-002.md"))
    );
    assert_eq!(
        edge.resolved_artifact,
        Some(ArtifactPath::new("acme/app", "adr-001.md"))
    );

    let _ = fs::remove_dir_all(root);
}
