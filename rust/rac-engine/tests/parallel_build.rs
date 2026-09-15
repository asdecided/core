//! Parallel cold build pins (INDEX-PLAN B5): worker-count invariance of the
//! store bytes, and the fault → serial-floor degrade. One #[test] because
//! cwd and the fault env var are process-global.

use std::fs;
use std::path::{Path, PathBuf};

use rac_engine::derived::SCHEMA_VERSION;
use rac_engine::index_store::{corpus_content_hash, store_dir, write_store};
use rac_engine::parallel_build::build_derived_index_parallel;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

fn store_hashes(cache_dir: &Path, corpus_hash: &str) -> Vec<(String, String)> {
    let seg_dir = store_dir(cache_dir, corpus_hash);
    let mut out: Vec<(String, String)> = fs::read_dir(seg_dir)
        .unwrap()
        .map(|e| {
            let e = e.unwrap();
            let bytes = fs::read(e.path()).unwrap();
            (
                e.file_name().to_string_lossy().into_owned(),
                rac_engine::sha256::hexdigest(&bytes),
            )
        })
        .collect();
    out.sort();
    out
}

#[test]
fn worker_count_is_invisible_in_the_store_bytes_and_faults_degrade() {
    std::env::set_current_dir(repo_root()).expect("chdir repo root");
    let directory = "rust/fixtures/index/repo/rac";
    let corpus_hash = corpus_content_hash(directory, true);

    let mut all_hashes = Vec::new();
    for workers in [1usize, 4usize] {
        let (derived, stats) = build_derived_index_parallel(directory, true, Some(workers));
        assert_eq!(stats.workers, workers);
        assert_eq!(stats.files, 11);
        let cache =
            std::env::temp_dir().join(format!("rac-pb-test-{workers}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&cache);
        assert!(write_store(&cache, &corpus_hash, SCHEMA_VERSION, &derived));
        all_hashes.push(store_hashes(&cache, &corpus_hash));
        let _ = fs::remove_dir_all(&cache);
    }
    assert_eq!(
        all_hashes[0], all_hashes[1],
        "store bytes must be worker-count-invariant"
    );

    // Fault → serial floor: the fan-out is discarded whole and the build
    // completes serially with identical structures.
    std::env::set_var("DECIDED_PARALLEL_BUILD_FAULT", "1");
    let (faulted, stats) = build_derived_index_parallel(directory, true, Some(4));
    std::env::remove_var("DECIDED_PARALLEL_BUILD_FAULT");
    assert_eq!(stats.workers, 1, "a fault must land on the serial floor");
    let cache = std::env::temp_dir().join(format!("rac-pb-test-fault-{}", std::process::id()));
    let _ = fs::remove_dir_all(&cache);
    assert!(write_store(&cache, &corpus_hash, SCHEMA_VERSION, &faulted));
    assert_eq!(
        store_hashes(&cache, &corpus_hash),
        all_hashes[0],
        "the serial floor must produce the same store"
    );
    let _ = fs::remove_dir_all(&cache);
}
