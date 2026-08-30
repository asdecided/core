use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use rac_engine::corpus::{ArtifactKey, Layer};
use rac_engine::derived_cache::{
    capture_logical_generation, FederatedCacheError, FederatedCacheRefresh,
    FederatedCacheTracker, LogicalGeneration, ReadModel,
};
use rac_engine::federation::{calculate_parent_digest, ParentCorpusErrorCode};
use rac_engine::index_store::{corpus_content_hash, store_dir, STORE_LAYOUT_VERSION};

static COUNTER: AtomicUsize = AtomicUsize::new(0);

const CHILD_DECISION: &str = r#"---
schema_version: 1
id: APP-KWJ4VMKVSS65
type: decision
---
# ADR-001: Child Policy

## Status

Accepted

## Context

The child needs one local decision.

## Decision

Keep the local layer.

## Consequences

The cache fixture is deterministic.

## Applies To

- src/**
"#;

const PARENT_DECISION: &str = r#"---
schema_version: 1
id: STD-KWJ4VMKVSS66
type: decision
---
# ADR-002: Parent Standard

## Status

Accepted

## Context

The parent needs one inherited decision.

## Decision

Keep the inherited layer.

## Consequences

The pin changes with these bytes.

## Applies To

- src/**
"#;

fn scratch(tag: &str) -> PathBuf {
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let root = std::env::temp_dir().join(format!(
        "asdecided-federated-cache-{tag}-{}-{n}",
        std::process::id()
    ));
    fs::create_dir_all(&root).unwrap();
    root
}

fn write_parent(root: &Path) {
    fs::create_dir_all(root.join(".decided")).unwrap();
    fs::create_dir_all(root.join("decisions")).unwrap();
    fs::write(
        root.join(".decided/config.yaml"),
        b"repository_key: STD\ncorpus:\n  source: acme/standards\n",
    )
    .unwrap();
    fs::write(root.join("decisions/parent.md"), PARENT_DECISION).unwrap();
}

fn write_manifest(child: &Path, pin: &str, override_note: &str) {
    fs::write(
        child.join(".decided/corpus.md"),
        format!(
            "# Corpus\n\n## inherits\n\n```yaml\nversion: 1\nalias: standards\nsource: acme/standards\nroot: vendor/standards\ncorpus: decisions\ndigest: {pin}\n```\n\n## overrides\n\n```yaml\nversion: 1\nitems: []\n```\n\n<!-- {override_note} -->\n"
        ),
    )
    .unwrap();
}

fn write_override_manifest(child: &Path, pin: &str) {
    fs::write(
        child.join(".decided/corpus.md"),
        format!(
            "# Corpus\n\n## inherits\n\n```yaml\nversion: 1\nalias: standards\nsource: acme/standards\nroot: vendor/standards\ncorpus: decisions\ndigest: {pin}\n```\n\n## overrides\n\n```yaml\nversion: 1\nitems:\n  - parent: standards::STD-KWJ4VMKVSS66\n    with: APP-KWJ4VMKVSS65\n    rationale: APP-KWJ4VMKVSS65\n```\n"
        ),
    )
    .unwrap();
}

fn fixture(tag: &str) -> (PathBuf, PathBuf, String) {
    let child = scratch(tag);
    let parent = child.join("vendor/standards");
    fs::create_dir_all(child.join(".decided")).unwrap();
    fs::create_dir_all(child.join("decisions")).unwrap();
    fs::write(
        child.join(".decided/config.yaml"),
        b"repository_key: APP\ncorpus:\n  source: acme/app\n",
    )
    .unwrap();
    fs::write(child.join("decisions/child.md"), CHILD_DECISION).unwrap();
    write_parent(&parent);
    let pin = calculate_parent_digest(&parent, "decisions")
        .unwrap()
        .digest;
    write_manifest(&child, &pin, "initial");
    (child, parent, pin)
}

fn child_corpus(child: &Path) -> String {
    child.join("decisions").to_string_lossy().into_owned()
}

fn shared_decision(id: &str) -> String {
    format!(
        "---\nschema_version: 1\nid: {id}\ntype: decision\n---\n# ADR-900: Shared Policy\n\n## Status\n\nAccepted\n\n## Context\n\nEqualmarker context.\n\n## Decision\n\nEqualmarker decision.\n\n## Consequences\n\nEqualmarker consequence.\n\n## Applies To\n\n- src/**\n"
    )
}

fn assert_digest_mismatch(error: FederatedCacheError) {
    match error {
        FederatedCacheError::Parent(error) => {
            assert_eq!(error.code, ParentCorpusErrorCode::DigestMismatch)
        }
        other => panic!("expected digest mismatch, got {other}"),
    }
}

fn assert_two_layers(model: &ReadModel) {
    let layers = match model {
        ReadModel::View(view) => view.layers().unwrap(),
        ReadModel::Fresh(derived) => derived.layers.clone(),
    };
    assert_eq!(layers.len(), 2);
    assert_eq!(layers[0].source, "acme/app");
    assert_eq!(layers[0].layer, Layer::Local);
    assert_eq!(layers[1].source, "acme/standards");
    assert_eq!(layers[1].layer, Layer::Inherited);
    assert!(layers[1].pin.as_deref().unwrap().starts_with("sha256:"));
    let qualified = model.resolve("standards::STD-KWJ4VMKVSS66");
    assert_eq!(qualified.outcome, rac_engine::resolve::OUTCOME_RESOLVED);
    assert_eq!(
        qualified.artifact.unwrap().key,
        Some(ArtifactKey::new("acme/standards", "STD-KWJ4VMKVSS66"))
    );
    let unqualified = model.resolve("STD-KWJ4VMKVSS66");
    assert_eq!(unqualified.outcome, rac_engine::resolve::OUTCOME_RESOLVED);
    assert_eq!(
        unqualified.artifact.unwrap().key,
        Some(ArtifactKey::new("acme/standards", "STD-KWJ4VMKVSS66"))
    );
}

fn assert_override_redirect(model: &ReadModel) {
    let qualified = model.resolve("standards::STD-KWJ4VMKVSS66");
    assert_eq!(qualified.outcome, rac_engine::resolve::OUTCOME_RESOLVED);
    assert_eq!(
        qualified.artifact.unwrap().key,
        Some(ArtifactKey::new("acme/standards", "STD-KWJ4VMKVSS66"))
    );
    let redirected = model.resolve("STD-KWJ4VMKVSS66");
    assert_eq!(redirected.outcome, rac_engine::resolve::OUTCOME_RESOLVED);
    assert_eq!(
        redirected.artifact.unwrap().key,
        Some(ArtifactKey::new("acme/app", "APP-KWJ4VMKVSS65"))
    );
    assert_eq!(
        model
            .canonical_redirect(&ArtifactKey::new("acme/standards", "STD-KWJ4VMKVSS66"))
            .unwrap()
            .unwrap()
            .replacement,
        ArtifactKey::new("acme/app", "APP-KWJ4VMKVSS65")
    );
}

fn full_entries(model: &ReadModel) -> Vec<rac_engine::resolve::IndexEntry> {
    match model {
        ReadModel::Fresh(derived) => derived.index_entries.clone(),
        ReadModel::View(reader) => (0..reader.doc_count)
            .map(|docid| reader.full_entry(docid).unwrap())
            .collect(),
    }
}

fn scope_rows(model: &ReadModel) -> Vec<rac_engine::retrieve::ScopeRow> {
    match model {
        ReadModel::Fresh(derived) => derived.scope_rows.clone(),
        ReadModel::View(reader) => reader.scope_rows().unwrap(),
    }
}

fn live_keys(model: &ReadModel) -> Vec<ArtifactKey> {
    match model {
        ReadModel::Fresh(derived) => derived.live_decision_keys.clone(),
        ReadModel::View(reader) => reader.live_decision_keys().unwrap(),
    }
}

fn portfolio(model: &ReadModel) -> serde_json::Value {
    match model {
        ReadModel::Fresh(derived) => derived.portfolio_summary.clone(),
        ReadModel::View(reader) => reader.portfolio_summary().unwrap(),
    }
}

fn search(model: &ReadModel, query: &str) -> rac_engine::resolve::SearchResult {
    match model {
        ReadModel::Fresh(derived) => {
            rac_engine::resolve::search_index(&derived.index_entries, query, None, &[])
        }
        ReadModel::View(reader) => {
            rac_engine::read_model::store_search(reader, query, None, &[], false)
        }
    }
}

#[test]
fn cold_warm_and_cross_process_store_hits_keep_source_provenance() {
    let (child, _parent, _pin) = fixture("cold-warm");
    let cache = scratch("cold-warm-cache");
    let directory = child_corpus(&child);
    let mut tracker = FederatedCacheTracker::new(cache.clone());

    let read = tracker.read_composed(&child, &directory, true).unwrap();
    assert_eq!(read.refresh, FederatedCacheRefresh::Recomposed);
    let identity = read.generation.identity().expect("federated identity");
    assert_eq!(identity.watched_roots.len(), 2);
    assert!(identity
        .watched_files
        .iter()
        .any(|path| path.ends_with(".decided/corpus.md")));
    assert!(identity
        .watched_files
        .iter()
        .any(|path| path.ends_with("decisions/parent.md")));
    assert_two_layers(read.model);
    let inherited_path = rac_engine::corpus::ArtifactPath::new("acme/standards", "parent.md");
    assert_eq!(
        read.inherited_bytes(&inherited_path),
        Some(PARENT_DECISION.as_bytes())
    );
    assert_eq!(
        read.inherited_text(&inherited_path).as_deref(),
        Some(PARENT_DECISION)
    );
    assert!(read.override_mapping().is_some());

    let read = tracker.read_composed(&child, &directory, true).unwrap();
    assert_eq!(read.refresh, FederatedCacheRefresh::WarmReuse);
    assert_two_layers(read.model);
    assert_eq!(
        read.inherited_bytes(&inherited_path),
        Some(PARENT_DECISION.as_bytes())
    );

    drop(tracker);
    let mut reopened = FederatedCacheTracker::new(cache.clone());
    let read = reopened.read_composed(&child, &directory, true).unwrap();
    assert_eq!(read.refresh, FederatedCacheRefresh::StoreHit);
    assert_two_layers(read.model);
    assert_eq!(
        read.inherited_bytes(&inherited_path),
        Some(PARENT_DECISION.as_bytes())
    );
    assert_eq!(read.composed.catalog().len(), 2);

    fs::remove_dir_all(child).unwrap();
    fs::remove_dir_all(cache).unwrap();
}

#[test]
fn every_composed_input_invalidates_and_an_invalid_parent_is_never_served() {
    let (child, parent, pin) = fixture("invalidation");
    let cache = scratch("invalidation-cache");
    let directory = child_corpus(&child);
    let mut tracker = FederatedCacheTracker::new(cache);

    let _ = tracker.read_composed(&child, &directory, true).unwrap();
    let first_key = tracker.current_key().unwrap().to_string();

    write_manifest(&child, &pin, "override-changed");
    let read = tracker.read_composed(&child, &directory, true).unwrap();
    assert_eq!(read.refresh, FederatedCacheRefresh::Recomposed);
    assert_ne!(tracker.current_key().unwrap(), first_key);

    fs::write(
        child.join(".decided/config.yaml"),
        b"repository_key: APP\ncorpus:\n  source: acme/app\n# child config input\n",
    )
    .unwrap();
    let before = tracker.current_key().unwrap().to_string();
    let _ = tracker.read_composed(&child, &directory, true).unwrap();
    assert_ne!(tracker.current_key().unwrap(), before);

    fs::write(
        child.join("decisions/child.md"),
        format!("{CHILD_DECISION}\n<!-- changed child corpus input -->\n"),
    )
    .unwrap();
    let before = tracker.current_key().unwrap().to_string();
    let _ = tracker.read_composed(&child, &directory, true).unwrap();
    assert_ne!(tracker.current_key().unwrap(), before);

    fs::write(
        parent.join("decisions/parent.md"),
        format!("{PARENT_DECISION}\n<!-- changed parent -->\n"),
    )
    .unwrap();
    let served_key = tracker.current_key().unwrap().to_string();
    let error = match tracker.read_composed(&child, &directory, true) {
        Err(error) => error,
        Ok(_) => panic!("stale parent bytes must not return the retained model"),
    };
    assert!(!error.to_string().contains(&child.to_string_lossy().to_string()));
    assert_digest_mismatch(error);
    assert_eq!(tracker.current_key().unwrap(), served_key);

    let repinned = calculate_parent_digest(&parent, "decisions")
        .unwrap()
        .digest;
    write_manifest(&child, &repinned, "override-changed");
    let read = tracker.read_composed(&child, &directory, true).unwrap();
    assert_eq!(read.refresh, FederatedCacheRefresh::Recomposed);

    fs::write(
        parent.join(".decided/config.yaml"),
        b"repository_key: STD\ncorpus:\n  source: acme/standards\n# parent config input\n",
    )
    .unwrap();
    let error = match tracker.read_composed(&child, &directory, true) {
        Err(error) => error,
        Ok(_) => panic!("stale parent config must not return the retained model"),
    };
    assert_digest_mismatch(error);
    let repinned = calculate_parent_digest(&parent, "decisions")
        .unwrap()
        .digest;
    write_manifest(&child, &repinned, "override-changed");
    let read = tracker.read_composed(&child, &directory, true).unwrap();
    assert_eq!(read.refresh, FederatedCacheRefresh::Recomposed);

    fs::remove_dir_all(child).unwrap();
}

#[test]
fn a_valid_v1_directory_is_an_explicit_miss_under_the_v2_layout() {
    assert_eq!(STORE_LAYOUT_VERSION, "v2");
    let (child, _parent, _pin) = fixture("layout-miss");
    let cache = scratch("layout-miss-cache");
    let directory = child_corpus(&child);
    let mut tracker = FederatedCacheTracker::new(cache.clone());
    let _ = tracker.read_composed(&child, &directory, true).unwrap();
    let key = tracker.current_key().unwrap().to_string();
    drop(tracker);

    let v2 = store_dir(&cache, &key);
    let v1 = cache.join("store/v1").join(&key);
    fs::create_dir_all(v1.parent().unwrap()).unwrap();
    fs::rename(&v2, &v1).unwrap();
    assert!(v1.is_dir());
    assert!(!v2.exists());

    let mut reopened = FederatedCacheTracker::new(cache.clone());
    let read = reopened.read_composed(&child, &directory, true).unwrap();
    assert_eq!(read.refresh, FederatedCacheRefresh::Recomposed);
    assert!(v1.is_dir(), "v1 remains disposable and is never decoded");
    assert!(store_dir(&cache, &key).is_dir());

    fs::remove_dir_all(child).unwrap();
    fs::remove_dir_all(cache).unwrap();
}

#[test]
fn shared_cache_reuses_clone_independent_source_relative_rows() {
    let (first, _first_parent, first_pin) = fixture("clone-a");
    let (second, _second_parent, second_pin) = fixture("clone-b");
    assert_eq!(first_pin, second_pin);
    let cache = scratch("clone-cache");
    let first_directory = child_corpus(&first);
    let second_directory = child_corpus(&second);

    let mut first_tracker = FederatedCacheTracker::new(cache.clone());
    let first_read = first_tracker
        .read_composed(&first, &first_directory, true)
        .unwrap();
    assert_eq!(first_read.refresh, FederatedCacheRefresh::Recomposed);
    let first_key = first_tracker.current_key().unwrap().to_string();
    drop(first_tracker);

    let mut second_tracker = FederatedCacheTracker::new(cache.clone());
    let second_read = second_tracker
        .read_composed(&second, &second_directory, true)
        .unwrap();
    assert_eq!(second_read.refresh, FederatedCacheRefresh::StoreHit);
    assert_eq!(second_read.generation.cache_key(), first_key);
    assert_eq!(portfolio(second_read.model)["directory"], "decisions");
    assert!(!portfolio(second_read.model)
        .to_string()
        .contains(&first.to_string_lossy().to_string()));
    for entry in full_entries(second_read.model) {
        assert!(matches!(entry.path.as_str(), "child.md" | "parent.md"));
        assert!(!entry.path.contains(&second.to_string_lossy().to_string()));
        let artifact_path = entry.artifact_path.expect("source-aware path");
        assert_eq!(entry.path, artifact_path.relative_path);
    }
    let rows = scope_rows(second_read.model);
    assert_eq!(rows.len(), 2);
    assert!(rows.iter().all(|row| {
        row.key.is_some()
            && row.origin.is_some()
            && row
                .artifact_path
                .as_ref()
                .is_some_and(|path| path.relative_path == row.path)
    }));
    let mut keys = live_keys(second_read.model);
    keys.sort();
    assert_eq!(
        keys,
        vec![
            ArtifactKey::new("acme/app", "APP-KWJ4VMKVSS65"),
            ArtifactKey::new("acme/standards", "STD-KWJ4VMKVSS66"),
        ]
    );

    fs::remove_dir_all(first).unwrap();
    fs::remove_dir_all(second).unwrap();
    fs::remove_dir_all(cache).unwrap();
}

#[test]
fn recursive_mode_and_exact_child_snapshot_are_generation_inputs() {
    let (child, _parent, _pin) = fixture("snapshot");
    fs::create_dir_all(child.join("decisions/nested")).unwrap();
    fs::write(child.join("decisions/nested/extra.md"), CHILD_DECISION).unwrap();
    let directory = child_corpus(&child);
    let recursive = capture_logical_generation(&child, &directory, true).unwrap();
    let top_level = capture_logical_generation(&child, &directory, false).unwrap();
    assert_ne!(recursive.cache_key(), top_level.cache_key());
    assert!(recursive.identity().unwrap().recursive);
    assert!(!top_level.identity().unwrap().recursive);
    assert_eq!(recursive.identity().unwrap().child_corpus_path, "decisions");

    fs::remove_file(child.join("decisions/nested/extra.md")).unwrap();
    let captured = capture_logical_generation(&child, &directory, true).unwrap();
    let first_key = captured.cache_key().to_string();
    fs::write(
        child.join("decisions/child.md"),
        CHILD_DECISION.replace("Child Policy", "Changed After Capture"),
    )
    .unwrap();
    let captured_composition = rac_engine::federated_corpus::compose_verified_generation_from_snapshot(
        &directory,
        captured.verified_parent().unwrap(),
        captured.child_files().unwrap(),
    )
    .unwrap();
    assert_eq!(
        captured_composition
            .resolve("APP-KWJ4VMKVSS65")
            .unwrap()
            .artifact
            .product
            .title
            .as_deref(),
        Some("ADR-001: Child Policy")
    );

    let cache = scratch("snapshot-cache");
    let mut tracker = FederatedCacheTracker::new(cache.clone());
    let read = tracker.read_composed(&child, &directory, true).unwrap();
    assert_eq!(read.refresh, FederatedCacheRefresh::Recomposed);
    assert_ne!(read.generation.cache_key(), first_key);
    let child_entry = full_entries(read.model)
        .into_iter()
        .find(|entry| entry.id == "APP-KWJ4VMKVSS65")
        .unwrap();
    assert_eq!(
        child_entry.title.as_deref(),
        Some("ADR-001: Changed After Capture")
    );

    fs::remove_dir_all(child).unwrap();
    fs::remove_dir_all(cache).unwrap();
}

#[test]
fn equal_public_paths_keep_source_ties_identical_cold_and_warm() {
    let (child, parent, _pin) = fixture("equal-path");
    fs::remove_file(child.join("decisions/child.md")).unwrap();
    fs::remove_file(parent.join("decisions/parent.md")).unwrap();
    fs::write(
        child.join("decisions/shared.md"),
        shared_decision("APP-KWJ4VMKVSS65"),
    )
    .unwrap();
    fs::write(
        parent.join("decisions/shared.md"),
        shared_decision("STD-KWJ4VMKVSS66"),
    )
    .unwrap();
    let pin = calculate_parent_digest(&parent, "decisions")
        .unwrap()
        .digest;
    write_manifest(&child, &pin, "equal-path");
    let directory = child_corpus(&child);
    let cache = scratch("equal-path-cache");

    let mut cold_tracker = FederatedCacheTracker::new(cache.clone());
    let cold = cold_tracker
        .read_composed(&child, &directory, true)
        .unwrap();
    let cold_sources: Vec<String> = search(cold.model, "equalmarker")
        .matches
        .into_iter()
        .map(|item| item.artifact_path.unwrap().source)
        .collect();
    assert_eq!(cold_sources, vec!["acme/app", "acme/standards"]);
    let rows = scope_rows(cold.model);
    assert_eq!(rows.len(), 2);
    assert!(rows.iter().all(|row| row.path == "shared.md"));
    assert_ne!(rows[0].key, rows[1].key);
    assert_eq!(live_keys(cold.model).len(), 2);
    drop(cold_tracker);

    let mut warm_tracker = FederatedCacheTracker::new(cache.clone());
    let warm = warm_tracker
        .read_composed(&child, &directory, true)
        .unwrap();
    assert_eq!(warm.refresh, FederatedCacheRefresh::StoreHit);
    let warm_sources: Vec<String> = search(warm.model, "equalmarker")
        .matches
        .into_iter()
        .map(|item| item.artifact_path.unwrap().source)
        .collect();
    assert_eq!(warm_sources, cold_sources);

    fs::remove_dir_all(child).unwrap();
    fs::remove_dir_all(cache).unwrap();
}

#[test]
fn override_redirects_are_identical_on_cold_and_store_hits() {
    let (child, _parent, pin) = fixture("override-redirect");
    let directory = child_corpus(&child);
    let cache = scratch("override-redirect-cache");
    write_override_manifest(&child, &pin);
    let mut tracker = FederatedCacheTracker::new(cache.clone());
    let cold = tracker.read_composed(&child, &directory, true).unwrap();
    assert_eq!(cold.composed.catalog().len(), 2);
    assert_eq!(cold.composed.effective().len(), 1);
    assert_override_redirect(cold.model);
    drop(tracker);

    let mut reopened = FederatedCacheTracker::new(cache.clone());
    let warm = reopened.read_composed(&child, &directory, true).unwrap();
    assert_eq!(warm.refresh, FederatedCacheRefresh::StoreHit);
    assert_override_redirect(warm.model);

    fs::remove_dir_all(child).unwrap();
    fs::remove_dir_all(cache).unwrap();
}

#[test]
fn no_manifest_keeps_the_single_corpus_content_key() {
    let root = scratch("legacy-key");
    fs::create_dir_all(root.join("decisions")).unwrap();
    fs::write(root.join("decisions/child.md"), CHILD_DECISION).unwrap();
    let directory = child_corpus(&root);
    let expected = corpus_content_hash(&directory, true);
    let generation = capture_logical_generation(&root, &directory, true).unwrap();
    assert!(matches!(generation, LogicalGeneration::Legacy { .. }));
    assert_eq!(generation.cache_key(), expected);
    assert!(generation.identity().is_none());
    assert!(generation.verified_parent().is_none());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn ancillary_readers_share_effective_and_local_composed_projections() {
    let (child, _parent, _pin) = fixture("ancillary-readers");
    let directory = child_corpus(&child);
    let corpus = rac_engine::federated_corpus::load_composed_corpus(&directory, true)
        .unwrap()
        .unwrap();
    let effective: Vec<_> = corpus.effective().cloned().collect();

    let index = rac_engine::index::build_repository_index_from_items(&directory, &effective, true);
    assert_eq!(index.artifacts.len(), 2);
    assert!(index.artifacts.iter().all(|row| row.origin.is_some()));
    let stats = rac_engine::stats::collect_stats_from_items(&directory, &effective);
    assert_eq!(stats.decisions.len(), 2);
    assert!(stats.decisions.iter().all(|row| row.origin.is_some()));

    let portfolio = rac_engine::portfolio::portfolio_from_composed(&directory, &corpus, true);
    assert_eq!(portfolio.total_artifacts(), 2);
    let coverage = rac_engine::coverage::analyze_coverage_from_composed(&directory, &corpus);
    assert_eq!(coverage.gaps.len(), 2);
    assert!(coverage.gaps.iter().all(|gap| gap.origin.is_some()));
    let relationships = rac_engine::relationships::build_relationship_report_from_composed(
        &directory,
        true,
        &corpus,
    );
    assert_eq!(relationships.total_files, 2);
    assert!(relationships
        .artifacts
        .iter()
        .all(|artifact| artifact.origin.is_some()));

    let doctor = rac_engine::doctor::diagnose_composed(&directory, true, 20, &corpus);
    assert!(doctor
        .findings
        .iter()
        .all(|finding| finding.path != "parent.md"));
    let review = rac_engine::review::build_review_composed(&directory, true, None, &corpus);
    assert_eq!(review.portfolio.total_artifacts(), 1);
    assert!(review.issues.iter().all(|issue| issue.path != "parent.md"));

    let herald = rac_engine::herald::collect_from_composed(
        &directory,
        &["src/main.rs".to_string()],
        &corpus,
    );
    let scope_rows = rac_engine::retrieve::scope_rows_from_items(&effective);
    assert_eq!(scope_rows.len(), 2);
    assert_eq!(scope_rows[0].scope_entries, vec!["src/**".to_string()]);
    assert_eq!(herald.decisions.len(), 2);
    let body = rac_engine::herald::render(&herald, "https://example.test/child", 10);
    assert!(body.contains("https://example.test/child/child.md"));
    assert!(!body.contains("https://example.test/child/parent.md"));

    let proposal = rac_engine::parse::parse_text(
        &CHILD_DECISION.replace(
            "## Consequences",
            "## Related Decisions\n\n- standards::STD-KWJ4VMKVSS66\n\n## Consequences",
        ),
        "-",
    );
    let validation = corpus.validate_proposed_document(&proposal, "-", &directory, true);
    assert!(validation.issues.is_empty());

    fs::remove_dir_all(child).unwrap();
}

#[test]
fn composed_portfolio_disambiguates_equal_relative_paths_by_source() {
    let (child, parent, _pin) = fixture("portfolio-equal-paths");
    fs::remove_file(child.join("decisions/child.md")).unwrap();
    fs::remove_file(parent.join("decisions/parent.md")).unwrap();
    fs::write(
        child.join("decisions/shared.md"),
        CHILD_DECISION.replace(
            "## Consequences",
            "## Related Decisions\n\n- APP-DOES-NOT-EXIST\n\n## Consequences",
        ),
    )
    .unwrap();
    fs::write(parent.join("decisions/shared.md"), PARENT_DECISION).unwrap();
    let pin = calculate_parent_digest(&parent, "decisions")
        .unwrap()
        .digest;
    write_manifest(&child, &pin, "equal-relative-paths");

    let directory = child_corpus(&child);
    let corpus = rac_engine::federated_corpus::load_composed_corpus(&directory, true)
        .unwrap()
        .unwrap();
    let portfolio = rac_engine::portfolio::portfolio_from_composed(&directory, &corpus, true);
    let relationship = portfolio
        .attention
        .iter()
        .find(|item| item.code == rac_engine::portfolio::ATTENTION_BROKEN_RELATIONSHIP)
        .expect("local broken relationship attention");
    assert_eq!(relationship.path, "shared.md");
    assert_eq!(relationship.identifier, "APP-KWJ4VMKVSS65");
    assert_eq!(
        relationship.origin.as_ref().map(|origin| origin.source.as_str()),
        Some("acme/app")
    );

    fs::remove_dir_all(child).unwrap();
}
