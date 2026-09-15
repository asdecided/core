//! FreshnessTracker state pins (INDEX-PLAN B6): cold start establishes the
//! mapped base; a change opens the delta window and serves the re-derived
//! snapshot; crossing the compaction threshold folds the window into a
//! fresh base, bumps the generation, and sheds the resident snapshot.

use std::fs;
use std::path::{Path, PathBuf};

use rac_engine::derived::{build_derived_index, SCHEMA_VERSION};
use rac_engine::freshness::{FreshnessTracker, TrackerModel};
use rac_engine::index_store::{store_dir, write_store};

fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rac-tracker-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("scratch");
    dir
}

const DOC: &str = "# ADR-1: Widget Base\n\n## Context\n\nBase.\n\n## Decision\n\nKeep.\n\n## Consequences\n\nNone.\n\n## Status\n\nAccepted\n";

#[test]
fn base_delta_compaction_lifecycle() {
    let corpus = scratch("corpus");
    let cache = scratch("cache");
    fs::write(corpus.join("adr-1-base.md"), DOC).unwrap();
    let root = corpus.to_string_lossy().into_owned();

    // Threshold 2: the second delta path triggers compaction — observable
    // without a 10k-file corpus.
    let mut tracker = FreshnessTracker::new_snapshot(cache.clone(), &root, Some(2));
    #[cfg(target_os = "linux")]
    assert_eq!(tracker.mode(), "inotify");
    #[cfg(not(target_os = "linux"))]
    assert_eq!(tracker.mode(), "stat");

    // Cold: full scan, first base written and served from the map.
    assert!(matches!(tracker.read_model(false), TrackerModel::View(_)));
    assert!(tracker.last_detect_scanned());
    assert_eq!(tracker.base_generation(), 1);
    assert_eq!(tracker.serving_generation(), 1);
    assert_eq!(tracker.delta_size(), 0);
    let cold_hash = tracker.corpus_hash().unwrap().to_string();

    // Unchanged corpus: the cached model is returned, nothing moves.
    assert!(matches!(tracker.read_model(false), TrackerModel::View(_)));
    #[cfg(target_os = "linux")]
    assert!(!tracker.last_detect_scanned());
    #[cfg(not(target_os = "linux"))]
    assert!(tracker.last_detect_scanned());
    assert_eq!(tracker.base_generation(), 1);
    assert_eq!(tracker.serving_generation(), 1);

    // One change: the delta window opens; serving switches to the
    // re-derived snapshot (no base rewrite below the threshold).
    fs::write(corpus.join("adr-2-two.md"), DOC.replace("ADR-1", "ADR-2")).unwrap();
    assert!(matches!(
        tracker.read_model(false),
        TrackerModel::Snapshot(_)
    ));
    assert!(tracker.last_detect_scanned());
    assert_eq!(tracker.base_generation(), 1);
    assert_eq!(tracker.serving_generation(), 2);
    assert_eq!(tracker.delta_size(), 1);
    assert_ne!(tracker.corpus_hash().unwrap(), cold_hash);

    // Second change crosses the threshold: compaction writes a fresh base,
    // bumps the generation, clears the window, sheds the snapshot.
    fs::write(corpus.join("adr-3-three.md"), DOC.replace("ADR-1", "ADR-3")).unwrap();
    assert!(matches!(tracker.read_model(false), TrackerModel::View(_)));
    assert_eq!(tracker.base_generation(), 2);
    assert_eq!(tracker.serving_generation(), 3);
    assert_eq!(tracker.delta_size(), 0);

    // A change after the shed re-parses on demand and still answers.
    fs::remove_file(corpus.join("adr-2-two.md")).unwrap();
    match tracker.read_model(false) {
        TrackerModel::Snapshot(derived) => {
            assert_eq!(derived.index_entries.len(), 2);
        }
        TrackerModel::View(_) => panic!("one change below threshold must serve the snapshot"),
        TrackerModel::Delta(_) => panic!("snapshot fallback must not serve delta generations"),
    }
    assert_eq!(tracker.serving_generation(), 4);

    let _ = fs::remove_dir_all(&corpus);
    let _ = fs::remove_dir_all(&cache);
}

#[test]
fn stat_fallback_and_verify_never_trust_clean_events() {
    let corpus = scratch("stat-corpus");
    let cache = scratch("stat-cache");
    fs::write(corpus.join("adr-1-base.md"), DOC).unwrap();
    let root = corpus.to_string_lossy().into_owned();
    let mut tracker = FreshnessTracker::new_stat(cache.clone(), &root, None);

    assert_eq!(tracker.mode(), "stat");
    tracker.read_model(false);
    assert!(tracker.last_detect_scanned());
    tracker.read_model(false);
    assert!(tracker.last_detect_scanned());

    let mut watcher = FreshnessTracker::new(cache.clone(), &root, None);
    watcher.read_model(false);
    watcher.read_model(true);
    assert!(watcher.last_detect_scanned());

    let _ = fs::remove_dir_all(&corpus);
    let _ = fs::remove_dir_all(&cache);
}

#[test]
fn detection_barrier_catches_immediate_nested_mutation() {
    let corpus = scratch("event-corpus");
    let cache = scratch("event-cache");
    fs::write(corpus.join("adr-1-base.md"), DOC).unwrap();
    let root = corpus.to_string_lossy().into_owned();
    let mut tracker = FreshnessTracker::new(cache.clone(), &root, Some(10));
    tracker.read_model(false);
    let generation = tracker.serving_generation();

    let nested = corpus.join("new").join("nested");
    fs::create_dir_all(&nested).unwrap();
    fs::write(nested.join("adr-2-two.md"), DOC.replace("ADR-1", "ADR-2")).unwrap();
    match tracker.read_model(false) {
        TrackerModel::Delta(delta) => {
            assert_eq!(
                delta.materialize_derived(&root, true).index_entries.len(),
                2
            )
        }
        TrackerModel::View(_) => panic!("nested mutation must open the delta window"),
        TrackerModel::Snapshot(_) => panic!("default tracker must serve the delta generation"),
    }
    assert!(tracker.last_detect_scanned());
    assert_eq!(tracker.serving_generation(), generation + 1);

    tracker.read_model(false);
    #[cfg(target_os = "linux")]
    assert!(!tracker.last_detect_scanned());
    #[cfg(not(target_os = "linux"))]
    assert!(tracker.last_detect_scanned());

    let _ = fs::remove_dir_all(&corpus);
    let _ = fs::remove_dir_all(&cache);
}

#[test]
fn watcher_setup_and_runtime_failure_degrade_to_stat() {
    let missing = scratch("missing-root");
    let _ = fs::remove_dir_all(&missing);
    let cache = scratch("missing-cache");
    let missing_root = missing.to_string_lossy().into_owned();
    let missing_tracker = FreshnessTracker::new(cache.clone(), &missing_root, None);
    assert_eq!(missing_tracker.mode(), "stat");

    let corpus = scratch("removed-corpus");
    fs::write(corpus.join("adr-1-base.md"), DOC).unwrap();
    let root = corpus.to_string_lossy().into_owned();
    let mut tracker = FreshnessTracker::new(cache.clone(), &root, Some(10));
    tracker.read_model(false);
    fs::remove_dir_all(&corpus).unwrap();
    match tracker.read_model(false) {
        TrackerModel::Delta(delta) => {
            assert!(delta
                .materialize_derived(&root, true)
                .index_entries
                .is_empty())
        }
        TrackerModel::View(_) => panic!("root removal must open an empty delta generation"),
        TrackerModel::Snapshot(_) => panic!("default tracker must serve the delta generation"),
    }
    assert_eq!(tracker.mode(), "stat");
    assert!(tracker.last_detect_scanned());

    let _ = fs::remove_dir_all(&cache);
}

fn store_hashes(cache_dir: &Path, corpus_hash: &str) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = fs::read_dir(store_dir(cache_dir, corpus_hash))
        .unwrap()
        .map(|entry| {
            let entry = entry.unwrap();
            let bytes = fs::read(entry.path()).unwrap();
            (
                entry.file_name().to_string_lossy().into_owned(),
                rac_engine::sha256::hexdigest(&bytes),
            )
        })
        .collect();
    out.sort();
    out
}

fn assert_delta_matches_fresh(model: &TrackerModel, root: &str, tag: &str) {
    let TrackerModel::Delta(generation) = model else {
        panic!("expected a delta generation");
    };
    let candidate_cache = scratch(&format!("{tag}-candidate"));
    let fresh_cache = scratch(&format!("{tag}-fresh"));
    let key = "referee";
    let materialized = generation.materialize_derived(root, true);
    assert!(write_store(
        &candidate_cache,
        key,
        SCHEMA_VERSION,
        &materialized,
    ));
    assert!(write_store(
        &fresh_cache,
        key,
        SCHEMA_VERSION,
        &build_derived_index(root, true),
    ));
    assert_eq!(
        store_hashes(&candidate_cache, key),
        store_hashes(&fresh_cache, key),
        "delta generation must be byte-identical to a fresh derivation"
    );
    let fresh_items = rac_engine::relationships::corpus_items(root, true);
    assert_eq!(
        generation.summary.value(root, true),
        rac_engine::output::portfolio_summary_value(&rac_engine::portfolio::portfolio_from_corpus(
            root,
            &fresh_items,
            true
        )),
        "summary generation must equal a fresh portfolio reduction"
    );
    let scope_signature = |rows: Vec<rac_engine::retrieve::ScopeRow>| {
        rows.into_iter()
            .map(|row| (row.id, row.title, row.status, row.path, row.scope_entries))
            .collect::<Vec<_>>()
    };
    assert_eq!(
        scope_signature(generation.scope.rows()),
        scope_signature(rac_engine::retrieve::scope_rows_from_items(&fresh_items)),
        "scope generation must equal fresh scope rows"
    );
    let fresh_live: Vec<String> = build_derived_index(root, true)
        .live_decision_paths
        .into_iter()
        .collect();
    assert_eq!(generation.scope.live_paths(), fresh_live);
    let relationship_signature = |edges: Vec<rac_engine::relationships::Relationship>| {
        edges
            .into_iter()
            .map(|edge| {
                (
                    edge.source_path,
                    edge.relationship,
                    edge.target,
                    edge.resolved_path,
                    edge.issue,
                )
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(
        relationship_signature(generation.graph.relationships()),
        relationship_signature(rac_engine::relationships::relationships_from_corpus(
            &fresh_items,
        )),
        "graph generation must equal fresh relationship resolution"
    );
    let fresh_index = rac_engine::resolve::index_from_items(&fresh_items);
    for entry in &fresh_index {
        for alias in &entry.aliases {
            let expected = rac_engine::resolve::resolve_in_index(&fresh_index, alias);
            let actual = generation.identity.resolve(alias);
            assert_eq!(
                actual.outcome, expected.outcome,
                "identity outcome for {alias}"
            );
            assert_eq!(actual.duplicate_paths, expected.duplicate_paths);
            assert_eq!(
                actual.artifact.as_ref().map(|artifact| (
                    artifact.id.as_str(),
                    artifact.artifact_type.as_str(),
                    artifact.title.as_deref(),
                    artifact.path.as_str(),
                )),
                expected.artifact.as_ref().map(|artifact| (
                    artifact.id.as_str(),
                    artifact.artifact_type.as_str(),
                    artifact.title.as_deref(),
                    artifact.path.as_str(),
                )),
                "identity row for {alias}"
            );
        }
    }
    let root_path = Path::new(root);
    for item in &fresh_items {
        let relative = Path::new(&item.path)
            .strip_prefix(root_path)
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let expected_status = rac_engine::resolve::artifact_status(&item.artifact);
        assert_eq!(
            generation.identity.status_for_path(&relative),
            Some(expected_status.as_str()),
            "status row for {relative}"
        );
    }
    for (query, artifact_type, live_only) in [
        ("widget", None, false),
        ("widget", None, true),
        ("adr widget", Some("decision"), false),
        ("widget widget", None, false),
        ("accepted", None, true),
        ("zzzz-no-match", None, false),
    ] {
        let expected = rac_engine::resolve::search_index_filtered(
            &fresh_index,
            query,
            artifact_type,
            &[],
            live_only,
        );
        let actual =
            generation
                .search
                .search(query, artifact_type, &[], live_only, &generation.graph);
        assert_eq!(
            rac_engine::output::search_result_value(&actual, true),
            rac_engine::output::search_result_value(&expected, true),
            "search generation for {query}"
        );
    }
    let _ = fs::remove_dir_all(candidate_cache);
    let _ = fs::remove_dir_all(fresh_cache);
}

#[test]
fn default_delta_stages_mutations_compacts_and_keeps_parsed_base() {
    let corpus = scratch("p6-corpus");
    let cache = scratch("p6-cache");
    fs::write(corpus.join("adr-1-base.md"), DOC).unwrap();
    fs::write(corpus.join("adr-2-two.md"), DOC.replace("ADR-1", "ADR-2")).unwrap();
    let root = corpus.to_string_lossy().into_owned();
    let mut tracker = FreshnessTracker::new(cache.clone(), &root, Some(3));

    assert!(tracker.delta_enabled());
    assert!(matches!(tracker.read_model(false), TrackerModel::View(_)));
    assert_eq!(tracker.base_generation(), 1);
    assert_eq!(tracker.delta_base_documents(), 2);
    assert_eq!(tracker.delta_size(), 0);
    assert_eq!(tracker.last_parse_files(), 2);

    fs::write(
        corpus.join("adr-1-base.md"),
        DOC.replace("Keep.", "Keep the edited base.")
            .replace("Accepted", "Superseded"),
    )
    .unwrap();
    let serving = tracker.serving_generation() + 1;
    let base = tracker.base_generation();
    let model = tracker.read_model(false);
    let TrackerModel::Delta(generation) = model else {
        panic!("one edit must open the delta generation");
    };
    assert_eq!(generation.base_generation, base);
    assert_eq!(generation.serving_generation, serving);
    assert_eq!(generation.changed_paths, vec!["adr-1-base.md"]);
    assert_delta_matches_fresh(model, &root, "p6-edit");
    assert_eq!(tracker.last_parse_files(), 1);
    assert_eq!(tracker.delta_upserts(), 1);
    assert_eq!(tracker.delta_tombstones(), 0);

    fs::remove_file(corpus.join("adr-2-two.md")).unwrap();
    let model = tracker.read_model(false);
    assert_delta_matches_fresh(model, &root, "p6-delete");
    assert_eq!(tracker.last_parse_files(), 0);
    assert_eq!(tracker.delta_upserts(), 1);
    assert_eq!(tracker.delta_tombstones(), 1);

    fs::rename(
        corpus.join("adr-1-base.md"),
        corpus.join("adr-3-renamed.md"),
    )
    .unwrap();
    assert!(matches!(tracker.read_model(false), TrackerModel::View(_)));
    assert_eq!(tracker.base_generation(), 2);
    assert_eq!(tracker.delta_base_documents(), 1);
    assert_eq!(tracker.delta_size(), 0);

    // Unlike the default ADR-107 snapshot shed, the first edit following a
    // P6 compaction parses only the changed document.
    fs::write(
        corpus.join("adr-3-renamed.md"),
        DOC.replace("Keep.", "Keep after compaction."),
    )
    .unwrap();
    let model = tracker.read_model(false);
    assert_delta_matches_fresh(model, &root, "p6-post-compact");
    assert_eq!(tracker.last_parse_files(), 1);
    assert_eq!(tracker.delta_upserts(), 1);
    assert_eq!(tracker.delta_tombstones(), 0);

    let _ = fs::remove_dir_all(&corpus);
    let _ = fs::remove_dir_all(&cache);
}
