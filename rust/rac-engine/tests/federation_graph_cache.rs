use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use rac_engine::derived_cache::{
    FederatedCacheError, FederatedCacheRefresh, GraphFederatedCacheTracker, ReadModel,
};
use rac_engine::derived::SCHEMA_VERSION;
use rac_engine::federation::{calculate_parent_digest_v2, ParentCorpusErrorCode};
use rac_engine::index_store::{graph_store_dir, open_graph_store, GRAPH_STORE_LAYOUT_VERSION};

static COUNTER: AtomicUsize = AtomicUsize::new(0);

const ROOT_SOURCE: &str = "acme/root";
const A_SOURCE: &str = "acme/a";
const B_SOURCE: &str = "acme/b";
const MID_SOURCE: &str = "acme/mid";
const BASE_SOURCE: &str = "acme/base";

#[derive(Clone)]
struct Parent<'a> {
    alias: &'a str,
    source: &'a str,
    root: &'a str,
    digest: &'a str,
}

fn scratch(tag: &str) -> PathBuf {
    let count = COUNTER.fetch_add(1, Ordering::SeqCst);
    let root = std::env::temp_dir().join(format!(
        "asdecided-graph-cache-{tag}-{}-{count}",
        std::process::id()
    ));
    fs::create_dir_all(&root).unwrap();
    root
}

fn decision(id: &str, label: &str) -> String {
    format!(
        "---\nschema_version: 1\nid: {id}\ntype: decision\n---\n# ADR-001: {label}\n\n## Status\n\nAccepted\n\n## Context\n\nThe graph cache fixture needs a valid decision.\n\n## Decision\n\nKeep {label} deterministic.\n\n## Consequences\n\nFreshness changes are observable.\n\n## Applies To\n\n- src/**\n"
    )
}

fn write_node(root: &Path, repository_key: &str, source: &str, id: &str, label: &str) {
    fs::create_dir_all(root.join(".decided")).unwrap();
    fs::create_dir_all(root.join("decisions")).unwrap();
    fs::write(
        root.join(".decided/config.yaml"),
        format!("repository_key: {repository_key}\ncorpus:\n  source: {source}\n"),
    )
    .unwrap();
    fs::write(root.join("decisions/policy.md"), decision(id, label)).unwrap();
}

fn write_manifest(root: &Path, parents: &[Parent<'_>]) {
    let mut text = String::from("# Corpus\n\n## inherits\n\n```yaml\nversion: 2\nparents:\n");
    for parent in parents {
        text.push_str(&format!(
            "  - alias: {}\n    source: {}\n    root: {}\n    corpus: decisions\n    digest: {}\n",
            parent.alias, parent.source, parent.root, parent.digest
        ));
    }
    text.push_str("```\n\n## overrides\n\n```yaml\nversion: 2\nitems: []\n```\n");
    fs::write(root.join(".decided/corpus.md"), text).unwrap();
}

fn pin(root: &Path) -> String {
    calculate_parent_digest_v2(root, "decisions")
        .unwrap()
        .digest
}

fn direct_fixture(tag: &str) -> (PathBuf, String, String) {
    let root = scratch(tag);
    let a = root.join("vendor/a");
    let b = root.join("vendor/b");
    write_node(&root, "APP", ROOT_SOURCE, "APP-KWJ4VMKVSS65", "root policy");
    write_node(&a, "AAA", A_SOURCE, "AAA-KWJ4VMKVSS66", "policy a");
    write_node(&b, "BBB", B_SOURCE, "BBB-KWJ4VMKVSS67", "policy b");
    let a_pin = pin(&a);
    let b_pin = pin(&b);
    write_manifest(
        &root,
        &[Parent {
            alias: "a",
            source: A_SOURCE,
            root: "vendor/a",
            digest: &a_pin,
        }],
    );
    (root, a_pin, b_pin)
}

fn chain_fixture(tag: &str) -> (PathBuf, PathBuf) {
    let root = scratch(tag);
    let mid = root.join("vendor/mid");
    let base = mid.join("vendor/base");
    write_node(&root, "APP", ROOT_SOURCE, "APP-KWJ4VMKVSS65", "root policy");
    write_node(&mid, "MID", MID_SOURCE, "MID-KWJ4VMKVSS68", "middle policy");
    write_node(&base, "BAS", BASE_SOURCE, "BAS-KWJ4VMKVSS69", "base policy");
    repin_chain(&root, &mid, &base);
    (root, base)
}

fn repin_chain(root: &Path, mid: &Path, base: &Path) {
    let base_pin = pin(base);
    write_manifest(
        mid,
        &[Parent {
            alias: "base",
            source: BASE_SOURCE,
            root: "vendor/base",
            digest: &base_pin,
        }],
    );
    let mid_pin = pin(mid);
    write_manifest(
        root,
        &[Parent {
            alias: "mid",
            source: MID_SOURCE,
            root: "vendor/mid",
            digest: &mid_pin,
        }],
    );
}

#[test]
fn cold_warm_and_cross_tracker_store_hit_use_one_exact_graph_generation() {
    let (root, _a_pin, _b_pin) = direct_fixture("cold-warm-store");
    let cache = scratch("cold-warm-store-cache");
    let mut first = GraphFederatedCacheTracker::new(cache.clone());

    let cold = first.read_graph(&root, "decisions", true, true).unwrap();
    assert_eq!(cold.refresh, FederatedCacheRefresh::Recomposed);
    assert!(cold.generation.starts_with("sha256-v3:"));
    assert_eq!(cold.metadata.generation, cold.generation);
    assert_eq!(cold.metadata.layers.len(), 2);
    assert!(matches!(cold.model, ReadModel::View(_)));
    let generation = cold.generation.to_string();
    assert!(graph_store_dir(&cache, &generation).unwrap().is_dir());

    let warm = first.read_graph(&root, "decisions", true, true).unwrap();
    assert_eq!(warm.refresh, FederatedCacheRefresh::WarmReuse);
    assert_eq!(warm.generation, generation);

    let mut second = GraphFederatedCacheTracker::new(cache.clone());
    {
        let stored = second.read_graph(&root, "decisions", true, true).unwrap();
        assert_eq!(stored.refresh, FederatedCacheRefresh::StoreHit);
        assert_eq!(stored.generation, generation);
        assert!(matches!(stored.model, ReadModel::View(_)));
    }
    let store = graph_store_dir(&cache, &generation).unwrap();
    drop(second);

    fs::write(store.join("graph.seg"), b"corrupt graph metadata").unwrap();
    let mut repaired = GraphFederatedCacheTracker::new(cache.clone());
    let repair = repaired.read_graph(&root, "decisions", true, true).unwrap();
    assert_eq!(repair.refresh, FederatedCacheRefresh::Recomposed);
    assert_eq!(repair.generation, generation);
    drop(repaired);

    let mut reopened = GraphFederatedCacheTracker::new(cache.clone());
    let stored = reopened.read_graph(&root, "decisions", true, true).unwrap();
    assert_eq!(stored.refresh, FederatedCacheRefresh::StoreHit);
    assert_eq!(stored.generation, generation);

    fs::remove_dir_all(root).unwrap();
    fs::remove_dir_all(cache).unwrap();
}

fn replace_once_same_length(path: &Path, from: &[u8], to: &[u8]) {
    assert_eq!(from.len(), to.len());
    let mut bytes = fs::read(path).unwrap();
    let offset = bytes
        .windows(from.len())
        .position(|window| window == from)
        .unwrap_or_else(|| panic!("{} omitted the corruption target", path.display()));
    bytes[offset..offset + from.len()].copy_from_slice(to);
    fs::write(path, bytes).unwrap();
}

#[test]
fn framing_valid_answer_corruption_is_removed_rebuilt_and_reopened() {
    let (root, _a_pin, _b_pin) = direct_fixture("semantic-corruption");
    let cache = scratch("semantic-corruption-cache");
    let mut initial = GraphFederatedCacheTracker::new(cache.clone());
    let (generation, metadata) = {
        let read = initial.read_graph(&root, "decisions", true, true).unwrap();
        assert_eq!(read.refresh, FederatedCacheRefresh::Recomposed);
        (read.generation.to_string(), read.metadata.clone())
    };
    drop(initial);

    let store = graph_store_dir(&cache, &generation).unwrap();
    let entries = store.join("entries.seg");
    replace_once_same_length(
        &entries,
        b"AAA-KWJ4VMKVSS66",
        b"ZAA-KWJ4VMKVSS66",
    );
    assert!(
        open_graph_store(&cache, &generation, SCHEMA_VERSION, &metadata).is_some(),
        "same-length answer mutation should preserve segment framing and graph metadata"
    );

    let mut repairing = GraphFederatedCacheTracker::new(cache.clone());
    let repaired = repairing
        .read_graph(&root, "decisions", true, true)
        .unwrap();
    assert_eq!(repaired.refresh, FederatedCacheRefresh::Recomposed);
    assert_eq!(repaired.generation, generation);
    drop(repairing);

    let repaired_entries = fs::read(&entries).unwrap();
    assert!(repaired_entries
        .windows(b"AAA-KWJ4VMKVSS66".len())
        .any(|window| window == b"AAA-KWJ4VMKVSS66"));
    assert!(!repaired_entries
        .windows(b"ZAA-KWJ4VMKVSS66".len())
        .any(|window| window == b"ZAA-KWJ4VMKVSS66"));

    let mut reopened = GraphFederatedCacheTracker::new(cache.clone());
    let stored = reopened.read_graph(&root, "decisions", true, true).unwrap();
    assert_eq!(stored.refresh, FederatedCacheRefresh::StoreHit);
    assert_eq!(stored.generation, generation);
    assert!(matches!(stored.model, ReadModel::View(_)));

    fs::remove_dir_all(root).unwrap();
    fs::remove_dir_all(cache).unwrap();
}

#[test]
fn cache_inside_read_only_materialisation_serves_fresh_without_writing() {
    let (root, _a_pin, _b_pin) = direct_fixture("unsafe-cache-root");
    let cache = root.join("vendor/a/.decided/cache");
    let mut tracker = GraphFederatedCacheTracker::new(cache.clone());

    let read = tracker.read_graph(&root, "decisions", true, true).unwrap();
    assert_eq!(read.refresh, FederatedCacheRefresh::Recomposed);
    assert!(matches!(read.model, ReadModel::Fresh(_)));
    assert!(
        !cache.exists(),
        "a graph cache path inside a read-only materialisation must not be created"
    );

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn ambiguous_nonexistent_parent_traversal_disables_graph_persistence() {
    let (root, _a_pin, _b_pin) = direct_fixture("ambiguous-cache-root");
    let cache = root.join("missing/../vendor/a/.decided/cache");
    let mut tracker = GraphFederatedCacheTracker::new(cache.clone());

    let read = tracker.read_graph(&root, "decisions", true, true).unwrap();
    assert_eq!(read.refresh, FederatedCacheRefresh::Recomposed);
    assert!(matches!(read.model, ReadModel::Fresh(_)));
    assert!(!root.join("missing").exists());
    assert!(!root.join("vendor/a/.decided/cache").exists());

    fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
#[test]
fn symlink_ancestor_into_parent_disables_graph_persistence() {
    use std::os::unix::fs::symlink;

    let (root, _a_pin, _b_pin) = direct_fixture("symlink-cache-root");
    let link = root.join("cache-link");
    symlink(root.join("vendor/a/.decided"), &link).unwrap();
    let cache = link.join("cache");
    let mut tracker = GraphFederatedCacheTracker::new(cache.clone());

    let read = tracker.read_graph(&root, "decisions", true, true).unwrap();
    assert_eq!(read.refresh, FederatedCacheRefresh::Recomposed);
    assert!(matches!(read.model, ReadModel::Fresh(_)));
    assert!(!root.join("vendor/a/.decided/cache").exists());

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn parent_tamper_fails_closed_and_drops_the_resident_model() {
    let (root, _a_pin, _b_pin) = direct_fixture("tamper");
    let cache = scratch("tamper-cache");
    let mut tracker = GraphFederatedCacheTracker::new(cache.clone());
    tracker.read_graph(&root, "decisions", true, true).unwrap();

    fs::write(
        root.join("vendor/a/decisions/policy.md"),
        decision("AAA-KWJ4VMKVSS66", "tampered policy a"),
    )
    .unwrap();
    let error = match tracker.read_graph(&root, "decisions", true, true) {
        Ok(_) => panic!("tampered closure must fail closed"),
        Err(error) => error,
    };
    match error {
        FederatedCacheError::Parent(error) => {
            assert_eq!(error.code, ParentCorpusErrorCode::DigestMismatch)
        }
        other => panic!("expected digest mismatch, got {other}"),
    }
    assert_eq!(tracker.current_generation(), None);

    fs::remove_dir_all(root).unwrap();
    fs::remove_dir_all(cache).unwrap();
}

#[test]
fn direct_parent_topology_and_manifest_permutation_fully_recompose() {
    let (root, a_pin, b_pin) = direct_fixture("topology-permutation");
    let cache = scratch("topology-permutation-cache");
    let mut tracker = GraphFederatedCacheTracker::new(cache.clone());

    let first_generation = tracker
        .read_graph(&root, "decisions", true, true)
        .unwrap()
        .generation
        .to_string();
    write_manifest(
        &root,
        &[
            Parent {
                alias: "a",
                source: A_SOURCE,
                root: "vendor/a",
                digest: &a_pin,
            },
            Parent {
                alias: "b",
                source: B_SOURCE,
                root: "vendor/b",
                digest: &b_pin,
            },
        ],
    );
    let topology = tracker.read_graph(&root, "decisions", true, true).unwrap();
    assert_eq!(topology.refresh, FederatedCacheRefresh::Recomposed);
    assert_ne!(topology.generation, first_generation);
    assert_eq!(topology.metadata.layers.len(), 3);
    let topology_generation = topology.generation.to_string();

    write_manifest(
        &root,
        &[
            Parent {
                alias: "b",
                source: B_SOURCE,
                root: "vendor/b",
                digest: &b_pin,
            },
            Parent {
                alias: "a",
                source: A_SOURCE,
                root: "vendor/a",
                digest: &a_pin,
            },
        ],
    );
    let permuted = tracker.read_graph(&root, "decisions", true, true).unwrap();
    assert_eq!(permuted.refresh, FederatedCacheRefresh::Recomposed);
    assert_ne!(permuted.generation, topology_generation);
    assert_eq!(permuted.metadata.layers.len(), 3);

    fs::remove_dir_all(root).unwrap();
    fs::remove_dir_all(cache).unwrap();
}

#[test]
fn transitive_leaf_change_reverified_and_repinned_through_the_whole_chain() {
    let (root, base) = chain_fixture("chain");
    let mid = root.join("vendor/mid");
    let cache = scratch("chain-cache");
    let mut tracker = GraphFederatedCacheTracker::new(cache.clone());
    let first = tracker.read_graph(&root, "decisions", true, true).unwrap();
    assert_eq!(first.metadata.layers.len(), 3);
    let first_generation = first.generation.to_string();

    fs::write(
        base.join("decisions/policy.md"),
        decision("BAS-KWJ4VMKVSS69", "updated base policy"),
    )
    .unwrap();
    repin_chain(&root, &mid, &base);
    let second = tracker.read_graph(&root, "decisions", true, true).unwrap();
    assert_eq!(second.refresh, FederatedCacheRefresh::Recomposed);
    assert_ne!(second.generation, first_generation);
    assert_eq!(second.metadata.layers.len(), 3);

    fs::remove_dir_all(root).unwrap();
    fs::remove_dir_all(cache).unwrap();
}

#[test]
fn cache_disabled_uses_the_same_verified_adapter_without_store_io() {
    let (root, _a_pin, _b_pin) = direct_fixture("cache-disabled");
    let cache = scratch("cache-disabled-cache");
    let mut tracker = GraphFederatedCacheTracker::new(cache.clone());
    let read = tracker.read_graph(&root, "decisions", true, false).unwrap();
    assert_eq!(read.refresh, FederatedCacheRefresh::Recomposed);
    assert!(matches!(read.model, ReadModel::Fresh(_)));
    assert!(read.generation.starts_with("sha256-v3:"));
    assert!(!cache
        .join("store")
        .join(GRAPH_STORE_LAYOUT_VERSION)
        .exists());

    fs::remove_dir_all(root).unwrap();
    fs::remove_dir_all(cache).unwrap();
}
