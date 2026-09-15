//! RAC-KXBPS7SRM6ZB REQ-004 — the differential-fuzz oracle-crash catalog
//! pinned as native regression fixtures.
//!
//! Every file under `rust/fixtures/hostile/` crashes the frozen Python
//! oracle with an uncaught traceback (PORT-CONTRACT decision 3: divergence
//! by design). These tests are NATIVE-ONLY — no oracle is involved — and
//! assert that the Rust engine stays total over the whole catalog: no
//! panic, graceful per-file issue reporting, and a corpus walk (including
//! the `rac new` id-collision walk, complementing
//! `scaffold::tests::new_survives_hostile_markdown_in_the_walk`) that
//! simply keeps going.

use rac_engine::commands::validate_directory;
use rac_engine::frontmatter::{file_cap_from, FileCap};
use rac_engine::parse;
use rac_engine::scaffold::create_artifact;
use rac_engine::validate::{has_errors, validate};

use std::path::{Path, PathBuf};

fn hostile_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../fixtures/hostile")
}

/// Every pinned fixture, sorted by name. Fails loudly if the set vanishes.
fn hostile_fixtures() -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(hostile_dir())
        .expect("rust/fixtures/hostile/ exists")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().map(|x| x == "md").unwrap_or(false))
        .collect();
    files.sort();
    assert!(
        files.len() >= 5,
        "hostile fixture set unexpectedly small: {} files",
        files.len()
    );
    files
}

fn scratch_root(tag: &str) -> PathBuf {
    let base = std::env::var("CARGO_TARGET_TMPDIR").unwrap_or_else(|_| "/tmp".into());
    let root = Path::new(&base).join(format!("hostile_{tag}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    root
}

/// Per-fixture: the parse/validate path is total and reports the class's
/// documented graceful outcome instead of raising.
#[test]
fn parse_and_validate_are_total_over_the_catalog() {
    for path in hostile_fixtures() {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let path_str = path.to_string_lossy().into_owned();
        let artifact = parse::parse_file(&path_str); // must not panic
        let issues = validate(&artifact, None, None); // must not panic

        if name.starts_with("class-a-") {
            // Unhashable YAML mapping keys: the healed oracle reports a
            // structured envelope failure instead of crashing, and the
            // native engine converges on the identical issue — the former
            // decision-3 divergence-marker class is retired here.
            let issue = issues
                .iter()
                .find(|i| i.code == "malformed-frontmatter")
                .unwrap_or_else(|| panic!("{name}: expected malformed-frontmatter"));
            assert_eq!(issue.severity, "error", "{name}: issue severity");
            assert!(
                issue
                    .message
                    .starts_with("frontmatter is not valid YAML: unhashable frontmatter key:"),
                "{name}: issue message was {:?}",
                issue.message
            );
        }
        if name.starts_with("class-c-") || name.starts_with("class-d-") {
            // Invalid UTF-8 on disk: lossy decode plus one warning, never
            // a raised decode error (the oracle's export re-read dies here).
            let warn = issues
                .iter()
                .find(|i| i.code == "non-utf8-content")
                .unwrap_or_else(|| panic!("{name}: expected non-utf8-content"));
            assert_eq!(warn.severity, "warning", "{name}: non-utf8 severity");
            assert!(
                !has_errors(&issues),
                "{name}: lossy decode must stay non-fatal"
            );
        }
        if name.starts_with("class-b-") {
            // The class-B crash is env-driven (see the cap-zone test); the
            // fixture itself is an ordinary valid artifact.
            assert!(!has_errors(&issues), "{name}: cap probe fixture is clean");
        }
    }
}

/// The directory walk (validate's uncached corpus path) visits every
/// hostile file and finishes with a per-file verdict for each.
#[test]
fn directory_validation_walks_the_whole_catalog() {
    let fixtures = hostile_fixtures();
    let root = scratch_root("walk");
    let corpus = root.join("corpus");
    std::fs::create_dir_all(&corpus).unwrap();
    for path in &fixtures {
        std::fs::copy(path, corpus.join(path.file_name().unwrap())).unwrap();
    }

    let result = validate_directory(&corpus.to_string_lossy(), true); // must not panic
    assert_eq!(
        result.files.len(),
        fixtures.len(),
        "every hostile file gets a per-file verdict"
    );
    for file in &result.files {
        assert!(
            ["valid", "invalid", "skipped"].contains(&file.status),
            "{}: unexpected status {:?}",
            file.path,
            file.status
        );
    }
    let _ = result.ok(); // verdict computes without panicking
    let _ = std::fs::remove_dir_all(&root);
}

/// RAC-KXBPS7SRM6ZB REQ-002 over the FULL catalog: `rac new` mints an id
/// even when the id-collision walk crosses every pinned crash class (the
/// oracle dies on the first one it parses).
#[test]
fn new_mints_an_id_over_the_full_catalog() {
    let root = scratch_root("new");
    std::fs::create_dir_all(root.join(".decided")).unwrap();
    std::fs::write(root.join(".decided/config.yaml"), "repository_key: RAC\n").unwrap();
    let corpus = root.join("rac/hostile");
    std::fs::create_dir_all(&corpus).unwrap();
    for path in hostile_fixtures() {
        std::fs::copy(&path, corpus.join(path.file_name().unwrap())).unwrap();
    }
    std::fs::create_dir_all(root.join("rac/decisions")).unwrap();

    let out = root
        .join("rac/decisions/minted.md")
        .to_string_lossy()
        .into_owned();
    let created = create_artifact("decision", &out).unwrap_or_else(|e| {
        panic!(
            "create_artifact failed on the hostile catalog: {}",
            e.message()
        )
    });
    assert_eq!(created.artifact_type, "decision");
    let written = std::fs::read_to_string(&out).unwrap();
    assert!(written.starts_with("---\nschema_version: 1\nid: RAC-"));
    let _ = std::fs::remove_dir_all(&root);
}

/// Class B — the two deterministic `DECIDED_MAX_FILE_BYTES` read-crash zones
/// classify as the graceful marker (never a panic, never an attempted
/// allocation), and a huge-but-parsed cap still reads every fixture.
#[test]
fn read_cap_crash_zones_stay_graceful() {
    assert_eq!(
        file_cap_from(Some("99999999999999999999")),
        FileCap::OracleCrash("OverflowError: cannot fit 'int' into an index-sized integer")
    );
    assert_eq!(
        file_cap_from(Some("9223372036854775806")),
        FileCap::OracleCrash("OverflowError: byte string is too large")
    );
    assert_eq!(file_cap_from(Some("1024")), FileCap::Cap(1024));

    // Below the crash zones the native engine reads incrementally; a cap of
    // 2^63 - 35 must parse every fixture without preallocating anything.
    let big_cap: u128 = (i64::MAX as u128) - 35;
    for path in hostile_fixtures() {
        let product = rac_engine::markdown::parse_file_with_cap(&path.to_string_lossy(), big_cap);
        assert_eq!(
            product.source_path,
            path.to_string_lossy(),
            "cap read returns the parsed product"
        );
    }
}

/// Class C — undecodable stdin bytes round through the surrogateescape
/// sentinel decode and the parse/validate pipeline without a panic (the
/// oracle dies in a later strict `str.encode`).
#[test]
fn stdin_surrogate_decode_is_total() {
    let raw = std::fs::read(hostile_dir().join("class-c-stdin-surrogate.md")).unwrap();
    assert!(
        String::from_utf8(raw.clone()).is_err(),
        "fixture must carry invalid UTF-8"
    );
    let text = rac_engine::pycompat::decode_stdin_surrogateescape(&raw);
    let artifact = parse::parse_text(&text, "-"); // must not panic
    let _ = validate(&artifact, None, None); // must not panic
}
