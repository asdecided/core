//! Git-recency conformance against a scratch repository with pinned
//! committer dates, so the test is a pure function of this file — no
//! dependence on the enclosing repo's history (which differs between a
//! working branch and a CI merge ref). The oracle equivalence of the
//! underlying semantics (verbatim %cI, timedelta.days flooring, strictly-
//! greater staleness) was pinned at port time via
//! rust/spec/gen_vectors_gitinfo.py against the Python engine.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use rac_engine::gitinfo::{
    last_committed, last_committed_for_paths, parse_iso8601_epoch, repository_root, staleness,
    DEFAULT_STALE_AFTER_DAYS,
};

/// Reference "now" for age math: 2027-01-01T00:00:00Z (matches the
/// convention the retired oracle-generated vectors used).
const REFERENCE_EPOCH: i64 = 1_798_761_600;

fn git(dir: &Path, args: &[&str], date: Option<&str>) {
    let mut cmd = Command::new("git");
    cmd.arg("-C").arg(dir).args(args);
    cmd.env("GIT_AUTHOR_NAME", "Vector Pin")
        .env("GIT_AUTHOR_EMAIL", "vector@example.invalid")
        .env("GIT_COMMITTER_NAME", "Vector Pin")
        .env("GIT_COMMITTER_EMAIL", "vector@example.invalid");
    if let Some(d) = date {
        cmd.env("GIT_AUTHOR_DATE", d).env("GIT_COMMITTER_DATE", d);
    }
    let out = cmd.output().expect("run git");
    assert!(
        out.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&out.stderr)
    );
}

fn scratch_repo() -> PathBuf {
    let base = std::env::var("CARGO_TARGET_TMPDIR").unwrap_or_else(|_| "/tmp".into());
    let dir = Path::new(&base).join(format!("gitinfo_scratch_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    git(&dir, &["init", "-q"], None);

    // committed.md: two commits; last one pinned at +01:00 on 2026-06-30.
    fs::write(dir.join("committed.md"), "# One\n").unwrap();
    git(&dir, &["add", "committed.md"], None);
    git(
        &dir,
        &["commit", "-q", "-m", "first"],
        Some("2026-06-01T09:00:00+01:00"),
    );
    fs::write(dir.join("committed.md"), "# One v2\n").unwrap();
    git(
        &dir,
        &["commit", "-q", "-am", "second"],
        Some("2026-06-30T23:30:00+01:00"),
    );

    // negative-offset.md: committer offset west of UTC, exercised verbatim.
    fs::write(dir.join("negative-offset.md"), "# West\n").unwrap();
    git(&dir, &["add", "negative-offset.md"], None);
    git(
        &dir,
        &["commit", "-q", "-m", "west"],
        Some("2026-12-31T20:00:00-07:00"),
    );

    // future.md: committed after REFERENCE_EPOCH -> negative age (floor
    // toward negative infinity, timedelta.days semantics). Non-UTC offset
    // deliberately: git >= 2.45 renders +00:00 as "Z" in %cI while older
    // git prints "+00:00" — both engines pass the same git's bytes through,
    // so parity holds either way, but a pinned literal must avoid UTC.
    fs::write(dir.join("future.md"), "# Future\n").unwrap();
    git(&dir, &["add", "future.md"], None);
    git(
        &dir,
        &["commit", "-q", "-m", "future"],
        Some("2027-01-03T06:00:00+02:00"),
    );

    // untracked.md: present on disk, never committed.
    fs::write(dir.join("untracked.md"), "# Loose\n").unwrap();
    dir
}

#[test]
fn recency_matches_pinned_semantics() {
    let repo = scratch_repo();
    let root = repository_root(&repo).expect("scratch repo has a root");

    // Verbatim %cI: committer offset preserved, never normalized to UTC.
    let last = last_committed(&root, &repo.join("committed.md"));
    assert_eq!(last.as_deref(), Some("2026-06-30T23:30:00+01:00"));

    // Age vs 2027-01-01T00:00:00Z: exact delta is 184d 1h 30m -> floors to 184.
    let st = staleness(last.as_deref(), DEFAULT_STALE_AFTER_DAYS, REFERENCE_EPOCH);
    assert_eq!(st.age_days, Some(184));
    assert_eq!(st.stale, Some(true), "184 > default threshold 180");
    assert_eq!(st.last_committed.as_deref(), last.as_deref());

    // Strictly-greater rule: age == threshold is NOT stale.
    assert_eq!(
        staleness(last.as_deref(), 184, REFERENCE_EPOCH).stale,
        Some(false)
    );
    assert_eq!(
        staleness(last.as_deref(), 183, REFERENCE_EPOCH).stale,
        Some(true)
    );

    // Negative committer offset stays verbatim; 2026-12-31T20:00-07:00 is
    // 2027-01-01T03:00Z -> negative exact delta floors to -1, not stale.
    let west = last_committed(&root, &repo.join("negative-offset.md"));
    assert_eq!(west.as_deref(), Some("2026-12-31T20:00:00-07:00"));
    let stw = staleness(west.as_deref(), DEFAULT_STALE_AFTER_DAYS, REFERENCE_EPOCH);
    assert_eq!(stw.age_days, Some(-1));
    assert_eq!(stw.stale, Some(false));

    // Future commit: 06:00+02:00 = 04:00Z -> -2d4h exact -> floors to -3.
    let fut = last_committed(&root, &repo.join("future.md"));
    assert_eq!(fut.as_deref(), Some("2027-01-03T06:00:00+02:00"));
    let stf = staleness(fut.as_deref(), DEFAULT_STALE_AFTER_DAYS, REFERENCE_EPOCH);
    assert_eq!(stf.age_days, Some(-3));
    assert_eq!(stf.stale, Some(false));

    // Untracked file: no commit, all-None staleness.
    let none = last_committed(&root, &repo.join("untracked.md"));
    assert_eq!(none, None);
    let stn = staleness(None, DEFAULT_STALE_AFTER_DAYS, REFERENCE_EPOCH);
    assert_eq!(stn.last_committed, None);
    assert_eq!(stn.age_days, None);
    assert_eq!(stn.stale, None);

    // The batched history traversal is byte-identical to the single-path
    // primitive and preserves input order, duplicates, and unknowns.
    let batch_paths = vec![
        repo.join("future.md"),
        repo.join("committed.md"),
        repo.join("untracked.md"),
        repo.join("committed.md"),
    ];
    let batch = last_committed_for_paths(&repo, &batch_paths);
    assert_eq!(batch[0].1.as_deref(), fut.as_deref());
    assert_eq!(batch[1].1.as_deref(), last.as_deref());
    assert_eq!(batch[2].1, None);
    assert_eq!(batch[3].1.as_deref(), last.as_deref());

    fs::remove_dir_all(&repo).ok();
}

#[test]
fn non_repo_directory_degrades_to_none() {
    // A temp dir outside any repo -> no root, all-None staleness.
    let base = std::env::var("CARGO_TARGET_TMPDIR").unwrap_or_else(|_| "/tmp".into());
    let dir = Path::new(&base).join(format!("gitinfo_norepo_{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    // /tmp on this box is not a git work tree; if it somehow is, skip.
    if repository_root(&dir).is_none() {
        let st = staleness(None, DEFAULT_STALE_AFTER_DAYS, 0);
        assert_eq!(st.stale, None);
    }
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn iso8601_parse_offsets() {
    // UTC epoch anchors, independent of the corpus.
    assert_eq!(parse_iso8601_epoch("1970-01-01T00:00:00+00:00"), Some(0));
    assert_eq!(parse_iso8601_epoch("1970-01-01T00:00:00Z"), Some(0));
    // +01:00 means local is one hour ahead of UTC -> epoch is one hour earlier.
    assert_eq!(
        parse_iso8601_epoch("1970-01-01T01:00:00+01:00"),
        Some(0),
        "offset must be subtracted to reach UTC"
    );
    assert_eq!(parse_iso8601_epoch("1970-01-01T00:00:00-01:00"), Some(3600));
    // Compact offset form.
    assert_eq!(parse_iso8601_epoch("1970-01-01T00:00:00+0000"), Some(0));
    // Fractional seconds are ignored for whole-day math.
    assert_eq!(
        parse_iso8601_epoch("1970-01-01T00:00:00.500000+00:00"),
        Some(0)
    );
    // Missing offset / garbage -> unknown.
    assert_eq!(parse_iso8601_epoch("not-a-date"), None);
    assert_eq!(parse_iso8601_epoch("1970-01-01T00:00:00"), None);
}
