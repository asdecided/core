//! Incremental-validate pins (INDEX-PLAN B4): the S5 accepted miss — an
//! in-place rewrite preserving BOTH size and mtime_ns is invisible to the
//! stat rung (the oracle's recorded behavior, pinned not fixed) — and the
//! `--verify` content-confirm floor that always catches it.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

use rac_engine::commands::validate_directory_incremental_in;

fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rac-inc-validate-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("scratch");
    dir
}

const INVALID: &str = "# ADR-9: Widget Probe\n\n## Context\n\nProbe body padded to length XX.\n\n## Decision\n\nProbe choice A.\n\n## Status\n\nProposed\n";
// Same byte length as INVALID, but now carries `## Consequences` — the
// validation outcome flips while the size stays identical.
const VALID: &str = "# ADR-9: Widget Probe\n\n## Context\n\nPxxxxxxxxxxxxxxxxxxxx.\n\n## Decision\n\nA.\n\n## Consequences\n\nOk.\n\n## Status\n\nProposed\n";

#[test]
fn s5_stat_preserving_rewrite_is_the_accepted_miss_and_verify_catches_it() {
    assert_eq!(INVALID.len(), VALID.len(), "the rewrite must preserve size");
    let root = scratch("s5-corpus");
    let cache = scratch("s5-cache");
    let file = root.join("adr-9-probe.md");
    fs::write(&file, INVALID).unwrap();

    let dir = root.to_string_lossy().into_owned();
    let first = validate_directory_incremental_in(&dir, true, false, Some(&cache));
    assert_eq!(first.files.len(), 1);
    assert_eq!(first.files[0].status, "invalid");

    // Preserve the stat pair across the rewrite: snapshot mtime, rewrite the
    // same number of bytes, restore mtime (ns) from the snapshot.
    let reference = root.join(".mtime-ref");
    let cp = Command::new("cp")
        .args(["-p", file.to_str().unwrap(), reference.to_str().unwrap()])
        .status()
        .unwrap();
    assert!(cp.success());
    fs::write(&file, VALID).unwrap();
    let touch = Command::new("touch")
        .args(["-r", reference.to_str().unwrap(), file.to_str().unwrap()])
        .status()
        .unwrap();
    assert!(touch.success());
    fs::remove_file(&reference).unwrap();

    // The stat rung reuses the stale row: the fixed file still reports
    // invalid — S5 is the ACCEPTED miss, pinned as-is.
    let stale = validate_directory_incremental_in(&dir, true, false, Some(&cache));
    assert_eq!(
        stale.files[0].status, "invalid",
        "S5 must be the accepted miss"
    );

    // The verify floor content-confirms every file and sees the fix.
    let verified = validate_directory_incremental_in(&dir, true, true, Some(&cache));
    assert_eq!(verified.files[0].status, "valid", "--verify must catch S5");

    // And the verify pass rewrote the store, so the plain stat rung now
    // agrees (self-healed).
    let after = validate_directory_incremental_in(&dir, true, false, Some(&cache));
    assert_eq!(after.files[0].status, "valid");

    let _ = fs::remove_dir_all(&root);
    let _ = fs::remove_dir_all(&cache);
}
