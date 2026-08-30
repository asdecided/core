//! Index-store vectors (INDEX-PLAN B2). The v2 source-aware layout retains
//! byte parity for unchanged v1 segments, pins deterministic whole-store
//! output across two writes, and round-trips provenance. The reader must fail
//! closed on every corruption class and reproduce a fresh build.

use std::fs;
use std::path::{Path, PathBuf};

use rac_engine::derived::{build_derived_index, SCHEMA_VERSION};
use rac_engine::index_store::{
    corpus_content_hash, decode_validation_store, encode_freshness_manifest,
    encode_validation_store, manifest_root_key, open_freshness_manifest, open_store,
    scoring_fingerprint, store_dir, write_freshness_manifest, write_store, CachedIssue,
    FileState, MmapIndexReader, ValidationCacheRow,
};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

fn vectors() -> serde_json::Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/vectors/index_store.json");
    serde_json::from_str(&fs::read_to_string(path).expect("vectors file")).expect("valid JSON")
}

fn scratch_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "rac-index-store-test-{tag}-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("scratch dir");
    dir
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn unhex(s: &str) -> Vec<u8> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).expect("hex"))
        .collect()
}

#[test]
fn fingerprint_matches_oracle() {
    assert_eq!(
        vectors()["scoring_fingerprint"].as_str().unwrap(),
        scoring_fingerprint()
    );
    assert_eq!(vectors()["bundle_version"].as_str().unwrap(), SCHEMA_VERSION);
}

/// One test drives every corpus golden: the store bytes embed the corpus
/// directory STRING, so the walk must run with the repo root as cwd and the
/// repo-relative directory — and cwd is process-global, so everything
/// cwd-dependent lives in this single test.
#[test]
fn v2_store_is_deterministic_and_retains_unchanged_segment_vectors() {
    std::env::set_current_dir(repo_root()).expect("chdir repo root");
    let vectors = vectors();
    for (name, corpus) in vectors["corpora"].as_object().unwrap() {
        let directory = corpus["directory"].as_str().unwrap();
        let expected_hash = corpus["corpus_hash"].as_str().unwrap();
        assert_eq!(
            corpus_content_hash(directory, true),
            expected_hash,
            "corpus hash diverged for {name} — regenerate the vectors after fixture changes"
        );

        let derived = build_derived_index(directory, true);
        let cache_dir = scratch_dir(&format!("golden-{name}"));
        assert!(write_store(&cache_dir, expected_hash, SCHEMA_VERSION, &derived));
        let seg_dir = store_dir(&cache_dir, expected_hash);

        let mut seen: Vec<String> = Vec::new();
        for entry in fs::read_dir(&seg_dir).unwrap() {
            seen.push(entry.unwrap().file_name().to_string_lossy().into_owned());
        }
        seen.sort();
        let golden = corpus["segments"].as_object().unwrap();
        let mut expected_names: Vec<String> = golden.keys().cloned().collect();
        expected_names.extend(
            [
                "artifactpathmap.seg",
                "identities.seg",
                "keymap.seg",
                "layers.seg",
                "redirects.seg",
            ]
                .into_iter()
                .map(str::to_string),
        );
        expected_names.sort();
        assert_eq!(seen, expected_names, "segment file set for {name}");

        for (seg, meta) in golden {
            if matches!(
                seg.as_str(),
                "entries.seg" | "relationships.seg" | "live.seg" | "scope.seg"
            ) {
                continue; // v2 appends source-aware identity to these rows
            }
            let bytes = fs::read(seg_dir.join(seg)).unwrap();
            assert_eq!(
                rac_engine::sha256::hexdigest(&bytes),
                meta["sha256"].as_str().unwrap(),
                "segment {seg} of {name} diverged from the oracle"
            );
            if let Some(raw) = meta["hex"].as_str() {
                assert_eq!(hex(&bytes), raw, "segment {seg} raw bytes of {name}");
            }
        }

        let second_cache = scratch_dir(&format!("golden-second-{name}"));
        assert!(write_store(
            &second_cache,
            expected_hash,
            SCHEMA_VERSION,
            &derived
        ));
        let second_dir = store_dir(&second_cache, expected_hash);
        for segment in &seen {
            assert_eq!(
                fs::read(seg_dir.join(segment)).unwrap(),
                fs::read(second_dir.join(segment)).unwrap(),
                "v2 segment {segment} of {name} is not deterministic"
            );
        }
        let _ = fs::remove_dir_all(&second_cache);

        // Reader round-trip: the mapped base reproduces the fresh build.
        let reader = open_store(&cache_dir, expected_hash, SCHEMA_VERSION).expect("open");
        reader_reproduces_fresh_build(&reader, &derived);

        // Warm == cold on the search surface, including the duplicate-token
        // class where the ORACLE's warm path diverges from its own cold path
        // (PORT-CONTRACT.d/10 §0a) — the native engine pins walk semantics
        // on both paths instead.
        let reader = open_store(&cache_dir, expected_hash, SCHEMA_VERSION).expect("reopen");
        for query in ["widget", "cache policy", "widget widget", "café", "sync sync sync"] {
            let cold = rac_engine::resolve::search_index_filtered(
                &derived.index_entries,
                query,
                None,
                &[],
                false,
            );
            let warm = rac_engine::read_model::store_search(&reader, query, None, &[], false);
            assert_eq!(
                rac_engine::output::render_find_json(&warm, true),
                rac_engine::output::render_find_json(&cold, true),
                "warm != cold for {query:?} over {name}"
            );
        }
        let cold = rac_engine::read_model::find_decisions_in_source_aware(
            &derived.index_entries,
            &derived.live_decision_keys,
            &derived.live_decision_paths,
            "widget",
        );
        let warm = rac_engine::read_model::store_find_decisions(&reader, "widget");
        assert_eq!(
            rac_engine::output::render_find_json(&warm, true),
            rac_engine::output::render_find_json(&cold, true),
            "decisions warm != cold over {name}"
        );
        drop(reader);

        // Fail-closed gates over this real store (LAST — the self-heal probe
        // rewrites the store with an empty bundle).
        corruption_gates(&cache_dir, expected_hash, &seg_dir);
        let _ = fs::remove_dir_all(&cache_dir);
    }
}

fn reader_reproduces_fresh_build(
    reader: &MmapIndexReader,
    derived: &rac_engine::derived::DerivedIndex,
) {
    assert_eq!(reader.doc_count as usize, derived.index_entries.len());
    assert_eq!(
        reader.identity_count().unwrap() as usize,
        derived.resolution.entries.len()
    );
    for (docid, entry) in derived.index_entries.iter().enumerate() {
        let docid = docid as u32;
        let full = reader.full_entry(docid).unwrap();
        assert_eq!(full.id, entry.id);
        assert_eq!(full.artifact_type, entry.artifact_type);
        assert_eq!(full.title, entry.title);
        assert_eq!(full.path, entry.path);
        assert_eq!(full.aliases, entry.aliases);
        assert_eq!(full.tags, entry.tags);
        assert_eq!(full.key, entry.key);
        assert_eq!(full.artifact_path, entry.artifact_path);
        assert_eq!(full.origin, entry.origin);
        assert_eq!(full.inbound_count, entry.inbound_count);
        assert_eq!(full.search_sections.len(), entry.search_sections.len());
        for (a, b) in full.search_sections.iter().zip(&entry.search_sections) {
            assert_eq!(a.heading, b.heading);
            assert_eq!(a.lines, b.lines);
        }
        assert_eq!(reader.entry_path(docid).unwrap(), entry.path);
        // Token vectors reconstruct in document order.
        let tokens = reader.field_tokens(docid).unwrap();
        let fresh = &derived.field_tokens[docid as usize];
        assert_eq!(tokens.id, fresh.id);
        assert_eq!(tokens.title, fresh.title);
        assert_eq!(tokens.path, fresh.path);
        assert_eq!(tokens.heading, fresh.heading);
        assert_eq!(tokens.body, fresh.body);
        assert_eq!(tokens.tags, fresh.tags);
        // Path map answers this doc.
        assert_eq!(reader.docid_for_path(&entry.path).unwrap(), Some(docid));
    }
    for (docid, entry) in derived.resolution.entries.iter().enumerate() {
        let docid = docid as u32;
        let identity = reader.identity_entry(docid).unwrap();
        assert_eq!(identity.id, entry.id);
        assert_eq!(identity.path, entry.path);
        assert_eq!(identity.aliases, entry.aliases);
        assert_eq!(identity.key, entry.key);
        assert_eq!(identity.artifact_path, entry.artifact_path);
        assert_eq!(identity.origin, entry.origin);
        let source = reader.source_artifact(docid).unwrap();
        assert_eq!(source.key, entry.key.clone().unwrap());
        assert_eq!(source.path, entry.artifact_path.clone().unwrap());
        assert!(reader.docids_for_key(&source.key).unwrap().contains(&docid));
        assert_eq!(reader.docid_for_artifact_path(&source.path).unwrap(), Some(docid));
    }
    assert_eq!(reader.layers().unwrap(), derived.layers);
    assert_eq!(
        reader.canonical_redirects().unwrap(),
        derived.resolution.canonical_redirects
    );
    // Relationships / live / scope / portfolio round-trip.
    let rels = reader.relationships().unwrap();
    assert_eq!(rels.len(), derived.relationships.len());
    for (a, b) in rels.iter().zip(&derived.relationships) {
        assert_eq!(a.source_path, b.source_path);
        assert_eq!(a.relationship, b.relationship);
        assert_eq!(a.target, b.target);
        assert_eq!(a.resolved_path, b.resolved_path);
        assert_eq!(a.issue, b.issue);
        assert_eq!(a.source_artifact, b.source_artifact);
        assert_eq!(a.resolved_artifact, b.resolved_artifact);
    }
    assert_eq!(
        reader.live_decision_paths().unwrap(),
        derived.live_decision_paths
    );
    assert_eq!(
        reader.live_decision_keys().unwrap(),
        derived.live_decision_keys
    );
    let scope = reader.scope_rows().unwrap();
    assert_eq!(scope.len(), derived.scope_rows.len());
    for (a, b) in scope.iter().zip(&derived.scope_rows) {
        assert_eq!(a.key, b.key);
        assert_eq!(a.artifact_path, b.artifact_path);
        assert_eq!(a.origin, b.origin);
        assert_eq!(a.id, b.id);
        assert_eq!(a.title, b.title);
        assert_eq!(a.status, b.status);
        assert_eq!(a.path, b.path);
        assert_eq!(a.scope_entries, b.scope_entries);
    }
    assert_eq!(reader.portfolio_summary().unwrap(), derived.portfolio_summary);
    assert_eq!(reader.docid_for_path("no/such/path.md").unwrap(), None);
    assert_eq!(
        reader
            .docid_for_key(&rac_engine::corpus::ArtifactKey::new("none/source", "none"))
            .unwrap(),
        None
    );
    assert!(reader.alias_docids("no-such-alias").unwrap().is_empty());
}

/// Every corruption class degrades to a miss on open — never a wrong answer.
fn corruption_gates(cache_dir: &Path, corpus_hash: &str, seg_dir: &Path) {
    // Wrong hash / wrong bundle version.
    assert!(open_store(cache_dir, "0".repeat(64).as_str(), SCHEMA_VERSION).is_none());
    assert!(open_store(cache_dir, corpus_hash, "2").is_none());

    let target = seg_dir.join("entries.seg");
    let original = fs::read(&target).unwrap();

    // Truncation (length gate).
    fs::write(&target, &original[..original.len() - 1]).unwrap();
    assert!(open_store(cache_dir, corpus_hash, SCHEMA_VERSION).is_none());
    // Trailing garbage.
    let mut long = original.clone();
    long.push(0);
    fs::write(&target, &long).unwrap();
    assert!(open_store(cache_dir, corpus_hash, SCHEMA_VERSION).is_none());
    // Bad magic.
    let mut bad_magic = original.clone();
    bad_magic[0] ^= 0xFF;
    fs::write(&target, &bad_magic).unwrap();
    assert!(open_store(cache_dir, corpus_hash, SCHEMA_VERSION).is_none());
    // Wrong format version.
    let mut bad_version = original.clone();
    bad_version[8] = 3;
    fs::write(&target, &bad_version).unwrap();
    assert!(open_store(cache_dir, corpus_hash, SCHEMA_VERSION).is_none());
    // Empty segment file.
    fs::write(&target, b"").unwrap();
    assert!(open_store(cache_dir, corpus_hash, SCHEMA_VERSION).is_none());
    // Missing segment file.
    fs::remove_file(&target).unwrap();
    assert!(open_store(cache_dir, corpus_hash, SCHEMA_VERSION).is_none());

    // Restore; must open again (the gates were the only obstacle).
    fs::write(&target, &original).unwrap();
    assert!(open_store(cache_dir, corpus_hash, SCHEMA_VERSION).is_some());

    // A same-hash rewrite over a CORRUPT store replaces it (self-heal).
    fs::write(&target, &original[..10]).unwrap();
    let derived = rac_engine::derived::DerivedIndex {
        layers: Vec::new(),
        source_artifacts: Vec::new(),
        resolution: Box::new(rac_engine::derived::ResolutionProjection {
            entries: Vec::new(),
            canonical_redirects: Vec::new(),
        }),
        index_entries: Vec::new(),
        field_tokens: Vec::new(),
        relationships: Vec::new(),
        live_decision_keys: Vec::new(),
        live_decision_paths: Vec::new(),
        portfolio_summary: serde_json::json!({}),
        scope_rows: Vec::new(),
    };
    // Rewriting with a mismatching (empty) bundle is fine for the probe —
    // content addressing only demands the final dir opens under the gates.
    assert!(write_store(cache_dir, corpus_hash, SCHEMA_VERSION, &derived));
    assert!(open_store(cache_dir, corpus_hash, SCHEMA_VERSION).is_some());
}

#[test]
fn vseg_bytes_match_oracle() {
    let vectors = vectors();
    let vseg = &vectors["vseg"];
    let config_hash = vseg["config_hash"].as_str().unwrap();
    let rows: Vec<(String, ValidationCacheRow)> = vseg["rows"]
        .as_object()
        .unwrap()
        .iter()
        .map(|(rel, row)| {
            (
                rel.clone(),
                ValidationCacheRow {
                    size: row["size"].as_u64().unwrap(),
                    mtime_ns: row["mtime_ns"].as_u64().unwrap(),
                    content_hash: row["content_hash"].as_str().unwrap().to_string(),
                    artifact_type: row["artifact_type"].as_str().unwrap().to_string(),
                    status: row["status"].as_str().unwrap().to_string(),
                    issues: row["issues"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|i| CachedIssue {
                            severity: i["severity"].as_str().unwrap().to_string(),
                            code: i["code"].as_str().unwrap().to_string(),
                            message: i["message"].as_str().unwrap().to_string(),
                            line: i["line"].as_u64().map(|l| l as u32),
                        })
                        .collect(),
                },
            )
        })
        .collect();
    let encoded = encode_validation_store(config_hash, &rows).unwrap();
    assert_eq!(hex(&encoded), vseg["hex"].as_str().unwrap());

    // Round-trip, plus the config-mismatch miss.
    let payload = rac_engine::index_format::segment_payload(&encoded).unwrap();
    let decoded = decode_validation_store(payload, config_hash).unwrap().unwrap();
    assert_eq!(decoded, rows);
    assert!(decode_validation_store(payload, "other-config")
        .unwrap()
        .is_none());
}

#[test]
fn fseg_bytes_match_oracle() {
    let vectors = vectors();
    let fseg = &vectors["fseg"];
    let manifest: Vec<(String, FileState)> = fseg["rows"]
        .as_object()
        .unwrap()
        .iter()
        .map(|(rel, row)| {
            (
                rel.clone(),
                FileState {
                    content_hash: row["content_hash"].as_str().unwrap().to_string(),
                    size: row["size"].as_u64().unwrap(),
                    mtime_ns: row["mtime_ns"].as_u64().unwrap(),
                },
            )
        })
        .collect();
    let encoded = encode_freshness_manifest(&manifest).unwrap();
    assert_eq!(hex(&encoded), fseg["hex"].as_str().unwrap());

    // Persisted round-trip through the atomic writer, plus corruption = miss.
    let cache_dir = scratch_dir("fseg");
    assert!(write_freshness_manifest(&cache_dir, "k1", &manifest));
    assert_eq!(open_freshness_manifest(&cache_dir, "k1").unwrap(), manifest);
    assert!(open_freshness_manifest(&cache_dir, "absent").is_none());
    let path = cache_dir.join("manifest/v1/k1.fseg");
    let bytes = fs::read(&path).unwrap();
    fs::write(&path, &bytes[..bytes.len() / 2]).unwrap();
    assert!(open_freshness_manifest(&cache_dir, "k1").is_none());
    let _ = fs::remove_dir_all(&cache_dir);
}

#[test]
fn manifest_root_key_matches_oracle() {
    let vectors = vectors();
    let probe = &vectors["manifest_root_key"];
    let dir = probe["directory"].as_str().unwrap();
    assert_eq!(
        manifest_root_key(dir, true),
        probe["recursive"].as_str().unwrap()
    );
    assert_eq!(
        manifest_root_key(dir, false),
        probe["top_level"].as_str().unwrap()
    );
}

#[test]
fn codec_rejects_bad_optional_flag_and_ranges() {
    use rac_engine::index_format::{encode_segment, segment_payload, Reader, Writer};
    let mut w = Writer::new();
    w.raw(&[7]); // invalid opt flag
    let payload = w.payload();
    let framed = encode_segment(&payload);
    let view = segment_payload(&framed).unwrap();
    assert_eq!(hex(view), hex(&payload));
    assert!(Reader::new(view).opt_text().is_err());

    // u32 writer range check degrades to an error, never wraps.
    let mut w = Writer::new();
    assert!(w.u32(u64::from(u32::MAX) + 1).is_err());

    // Reads past the end fail.
    assert!(Reader::new(&[1, 2]).u32().is_err());
    assert!(Reader::new(&unhex("04000000aabb")).blob().is_err());
}
