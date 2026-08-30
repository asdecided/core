//! Black-box guards for historical exports that cannot reproduce the
//! governing configuration or committed bytes from the selected repository.

use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static SCRATCH_SEQUENCE: AtomicU64 = AtomicU64::new(0);

fn scratch_suffix() -> String {
    let sequence = SCRATCH_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock after epoch")
        .as_nanos();
    format!("{}-{nonce}-{sequence}", std::process::id())
}

struct TestRepo {
    scratch: PathBuf,
    root: PathBuf,
}

impl TestRepo {
    fn new(label: &str) -> Self {
        Self::new_with_git_directory(label, false).0
    }

    fn new_with_external_git_directory(label: &str) -> (Self, PathBuf) {
        Self::new_with_git_directory(label, true)
    }

    fn new_with_git_directory(label: &str, separate_git_directory: bool) -> (Self, PathBuf) {
        let scratch =
            std::env::temp_dir().join(format!("asdecided-at-guard-{label}-{}", scratch_suffix()));
        let root = scratch.join("repository");
        fs::create_dir_all(&root).expect("create point-in-time guard repository");

        let repo = Self { scratch, root };
        let resolved_git_directory = if separate_git_directory {
            repo.scratch.join("external.git")
        } else {
            repo.root.join(".git")
        };
        if separate_git_directory {
            let git_directory = resolved_git_directory.to_string_lossy().into_owned();
            repo.git(&[
                "init",
                "--quiet",
                "--separate-git-dir",
                git_directory.as_str(),
            ]);
        } else {
            repo.git(&["init", "--quiet"]);
        }
        repo.git(&["config", "user.name", "AsDecided Test"]);
        repo.git(&["config", "user.email", "test@asdecided.invalid"]);
        repo.git(&["config", "core.autocrlf", "false"]);
        (repo, resolved_git_directory)
    }

    fn path(&self, relative: &str) -> PathBuf {
        self.root.join(relative)
    }

    fn write(&self, relative: &str, contents: &str) {
        let path = self.path(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create point-in-time guard fixture directory");
        }
        fs::write(path, contents).expect("write point-in-time guard fixture");
    }

    fn git(&self, args: &[&str]) -> String {
        let output = Command::new("git")
            .args(args)
            .current_dir(&self.root)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .expect("run git for point-in-time guard fixture");
        assert!(
            output.status.success(),
            "git {args:?} failed with {:?}\nstdout:\n{}\nstderr:\n{}",
            output.status.code(),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }

    fn commit_all(&self) {
        self.git(&["add", "--all"]);
        self.git(&[
            "commit",
            "--quiet",
            "--message",
            "test: point-in-time materialization guard",
        ]);
    }

    fn run(&self, corpus: &Path, revision: Option<&str>) -> Output {
        self.run_with_temp(corpus, revision, None)
    }

    fn run_with_temp(&self, corpus: &Path, revision: Option<&str>, temp: Option<&Path>) -> Output {
        let corpus = corpus.to_string_lossy().into_owned();
        let mut args = vec!["export", corpus.as_str(), "--documents"];
        if let Some(revision) = revision {
            args.extend(["--at", revision]);
        }
        let mut command = Command::new(env!("CARGO_BIN_EXE_decided"));
        command
            .args(args)
            .current_dir(&self.root)
            .env("DECIDED_CACHE_DIR", self.scratch.join("cache"))
            .env("XDG_CACHE_HOME", self.scratch.join("xdg-cache"))
            .env("XDG_CONFIG_HOME", self.scratch.join("xdg-config"))
            .env("XDG_STATE_HOME", self.scratch.join("xdg-state"));
        if let Some(temp) = temp {
            command.env("TMPDIR", temp);
        }
        command
            .output()
            .expect("run decided point-in-time guard fixture")
    }
}

impl Drop for TestRepo {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.scratch);
    }
}

fn decision(extra: &str) -> String {
    format!(
        "---\nschema_version: 1\nid: PIT-01K000000099\ntype: decision\n---\n# Guarded Historical Export\n\n## Context\n\nHistorical bytes must be exact.\n\n## Decision\n\nKeep materialization strict. {extra}\n\n## Consequences\n\nInexact snapshots fail closed.\n\n## Status\n\nAccepted\n"
    )
}

fn assert_usage_error(output: &Output, expected: &[&str]) {
    assert_eq!(
        output.status.code(),
        Some(2),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.stdout.is_empty(),
        "guard failure emitted a partial export: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    for fragment in expected {
        assert!(
            stderr.contains(fragment),
            "missing {fragment:?} in actionable usage error: {stderr}"
        );
    }
}

#[test]
fn governing_config_above_git_top_level_is_rejected() {
    let repo = TestRepo::new("outside-governance");
    fs::create_dir_all(repo.scratch.join(".decided")).expect("create governing config directory");
    fs::write(
        repo.scratch.join(".decided/config.yaml"),
        "repository_key: OUT\ncorpus:\n  source: tests/outside-governance\n",
    )
    .expect("write governing config outside repository");
    repo.write("decisions/decision.md", &decision(""));
    repo.commit_all();

    let output = repo.run(&repo.path("decisions"), Some("HEAD"));

    assert_usage_error(&output, &[".decided/config.yaml", "outside", "repository"]);
}

#[test]
fn historical_in_repository_config_wins_over_a_current_outside_ancestor() {
    let repo = TestRepo::new("historical-in-repository-governance");
    repo.write(
        ".decided/config.yaml",
        "repository_key: PIT\ncorpus:\n  source: tests/historical-governance\n",
    );
    repo.write("decisions/decision.md", &decision("HISTORICAL GOVERNANCE"));
    repo.commit_all();
    let historical = repo.git(&["rev-parse", "HEAD"]);

    fs::remove_file(repo.path(".decided/config.yaml")).expect("remove current repository config");
    fs::create_dir_all(repo.scratch.join(".decided")).expect("create outside config directory");
    fs::write(
        repo.scratch.join(".decided/config.yaml"),
        "repository_key: OUT\ncorpus:\n  source: tests/outside-governance\n",
    )
    .expect("write outside current config");

    let output = repo.run(&repo.path("decisions"), Some(&historical));
    assert!(
        output.status.success(),
        "historical in-repository config was shadowed by current outside config: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("tests/historical-governance"),
        "stdout={stdout}"
    );
    assert!(
        !stdout.contains("tests/outside-governance"),
        "stdout={stdout}"
    );
}

#[test]
fn temporary_directory_ancestor_config_cannot_inject_historical_identity() {
    let repo = TestRepo::new("temporary-ancestor-boundary");
    repo.write("decisions/decision.md", &decision("BOUNDED SNAPSHOT"));
    repo.commit_all();
    let ambient = repo.scratch.join("ambient-temp");
    fs::create_dir_all(ambient.join(".decided")).expect("create ambient temp config directory");
    fs::write(
        ambient.join(".decided/config.yaml"),
        "repository_key: AMBIENT\ncorpus:\n  source: tests/ambient-temp\n",
    )
    .expect("write ambient temp config");

    let output = repo.run_with_temp(&repo.path("decisions"), Some("HEAD"), Some(&ambient));
    assert!(
        output.status.success(),
        "ambient temp config disrupted historical export: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(!stdout.contains("tests/ambient-temp"), "stdout={stdout}");
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse viewer export");
    assert_eq!(
        payload["metadata"]["source"], "decisions",
        "stdout={stdout}"
    );
}

#[cfg(unix)]
#[test]
fn snapshot_temp_base_inside_repository_is_rejected_before_any_write() {
    let repo = TestRepo::new("repository-temp-boundary");
    repo.write(
        "decisions/decision.md",
        &decision("NO REPOSITORY TEMP WRITE"),
    );
    repo.commit_all();

    let output = repo.run_with_temp(&repo.path("decisions"), Some("HEAD"), Some(&repo.root));
    assert_eq!(output.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("revision snapshot base must be outside the selected repository"),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let leaked = fs::read_dir(&repo.root)
        .expect("read repository root")
        .filter_map(Result::ok)
        .any(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with("decided-revision-")
        });
    assert!(
        !leaked,
        "snapshot temp directory was created inside repository"
    );
}

#[cfg(unix)]
#[test]
fn snapshot_temp_base_inside_external_git_directory_is_rejected_before_any_write() {
    let (repo, git_directory) = TestRepo::new_with_external_git_directory("external-git-temp");
    repo.write(
        "decisions/decision.md",
        &decision("NO EXTERNAL GIT TEMP WRITE"),
    );
    repo.commit_all();

    let output = repo.run_with_temp(&repo.path("decisions"), Some("HEAD"), Some(&git_directory));
    assert_eq!(output.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("revision snapshot base must be outside the selected Git directory"),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let leaked = fs::read_dir(&git_directory)
        .expect("read external Git directory")
        .filter_map(Result::ok)
        .any(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with("decided-revision-")
        });
    assert!(
        !leaked,
        "snapshot temp directory was created inside external Git directory"
    );
}

fn assert_export_attribute_preserves_bytes(attribute: &str, extra: &str) {
    let repo = TestRepo::new(attribute);
    repo.write(
        ".decided/config.yaml",
        "repository_key: PIT\ncorpus:\n  source: tests/materialization-guards\n",
    );
    repo.write("decisions/decision.md", &decision(extra));
    repo.write(
        ".gitattributes",
        &format!("decisions/decision.md {attribute}\n"),
    );
    repo.commit_all();

    let plain = repo.run(&repo.path("decisions"), None);
    let historical = repo.run(&repo.path("decisions"), Some("HEAD"));

    for (output, context) in [(&plain, "plain export"), (&historical, "historical export")] {
        assert!(
            output.status.success(),
            "{context} with {attribute} failed with {:?}\nstdout:\n{}\nstderr:\n{}",
            output.status.code(),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    assert_eq!(
        historical.stdout, plain.stdout,
        "{attribute} changed committed bytes in the historical export"
    );
    assert!(
        String::from_utf8_lossy(&historical.stdout).contains(extra),
        "{attribute} export did not preserve the exact corpus marker {extra:?}"
    );
}

#[test]
fn export_ignore_on_a_corpus_path_does_not_omit_committed_bytes() {
    assert_export_attribute_preserves_bytes("export-ignore", "EXPORT IGNORE BODY");
}

#[test]
fn export_subst_on_a_corpus_path_does_not_rewrite_committed_bytes() {
    assert_export_attribute_preserves_bytes("export-subst", "$Format:%H$");
}
