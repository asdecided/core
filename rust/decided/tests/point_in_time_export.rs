//! Black-box contract tests for `decided export --at <rev>`.

use serde_json::Value;
use std::collections::BTreeMap;
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
    root: PathBuf,
    runtime: PathBuf,
}

impl TestRepo {
    fn new(label: &str) -> Self {
        let suffix = scratch_suffix();
        let root = std::env::temp_dir().join(format!("asdecided-at-{label}-{suffix}"));
        let runtime = std::env::temp_dir().join(format!("asdecided-at-{label}-runtime-{suffix}"));
        fs::create_dir_all(&root).expect("create point-in-time repository");
        fs::create_dir_all(&runtime).expect("create point-in-time runtime directory");

        let repo = Self { root, runtime };
        repo.git(&["init", "--quiet"]);
        repo.git(&["config", "user.name", "AsDecided Test"]);
        repo.git(&["config", "user.email", "test@asdecided.invalid"]);
        repo.git(&["config", "core.autocrlf", "false"]);
        repo
    }

    fn path(&self, relative: &str) -> PathBuf {
        self.root.join(relative)
    }

    fn write(&self, relative: &str, contents: &str) {
        let path = self.path(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create fixture parent directory");
        }
        fs::write(path, contents).expect("write point-in-time fixture");
    }

    fn configure_corpus(&self) {
        self.configure_corpus_source("tests/point-in-time");
    }

    fn configure_corpus_source(&self, source: &str) {
        self.write(
            ".decided/config.yaml",
            &format!("repository_key: PIT\ncorpus:\n  source: {source}\n"),
        );
    }

    fn git_output(&self, args: &[&str]) -> Output {
        Command::new("git")
            .args(args)
            .current_dir(&self.root)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .expect("run git for point-in-time fixture")
    }

    fn git(&self, args: &[&str]) -> String {
        let output = self.git_output(args);
        assert!(
            output.status.success(),
            "git {args:?} failed with {:?}\nstdout:\n{}\nstderr:\n{}",
            output.status.code(),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }

    fn commit_all(&self, message: &str) -> String {
        self.git(&["add", "--all"]);
        self.git(&["commit", "--quiet", "--message", message]);
        self.git(&["rev-parse", "HEAD"])
    }

    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_decided"))
            .args(args)
            .current_dir(&self.root)
            .env("DECIDED_CACHE_DIR", self.runtime.join("decided-cache"))
            .env("XDG_CACHE_HOME", self.runtime.join("xdg-cache"))
            .env("XDG_CONFIG_HOME", self.runtime.join("xdg-config"))
            .env("XDG_STATE_HOME", self.runtime.join("xdg-state"))
            .output()
            .expect("run decided point-in-time export")
    }

    fn status(&self) -> Vec<u8> {
        self.git_output(&["status", "--porcelain=v1", "--untracked-files=all"])
            .stdout
    }

    fn worktree_snapshot(&self) -> BTreeMap<PathBuf, Vec<u8>> {
        fn visit(root: &Path, directory: &Path, files: &mut BTreeMap<PathBuf, Vec<u8>>) {
            let mut entries: Vec<_> = fs::read_dir(directory)
                .expect("read point-in-time worktree")
                .map(|entry| entry.expect("read point-in-time entry"))
                .collect();
            entries.sort_by_key(|entry| entry.file_name());
            for entry in entries {
                let path = entry.path();
                if path == root.join(".git") {
                    continue;
                }
                let file_type = entry.file_type().expect("inspect point-in-time entry");
                if file_type.is_dir() {
                    visit(root, &path, files);
                } else if file_type.is_file() {
                    files.insert(
                        path.strip_prefix(root)
                            .expect("fixture path is beneath root")
                            .to_path_buf(),
                        fs::read(path).expect("read point-in-time fixture file"),
                    );
                }
            }
        }

        let mut files = BTreeMap::new();
        visit(&self.root, &self.root, &mut files);
        files
    }
}

impl Drop for TestRepo {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
        let _ = fs::remove_dir_all(&self.runtime);
    }
}

fn decision(marker: &str) -> String {
    format!(
        "---\nschema_version: 1\nid: PIT-01K000000001\ntype: decision\n---\n# Point-in-Time Decision\n\n## Context\n\nHistorical exports must be reproducible.\n\n## Decision\n\n{marker}\n\n## Consequences\n\nConsumers can recover the selected state.\n\n## Status\n\nAccepted\n"
    )
}

fn assert_success(output: &Output, context: &str) {
    assert!(
        output.status.success(),
        "{context} failed with {:?}\nstdout:\n{}\nstderr:\n{}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn documents_export_selects_a_or_b_byte_stably_without_mutating_git() {
    let repo = TestRepo::new("documents");
    repo.configure_corpus_source("tests/point-in-time-a");
    repo.write("decisions/point-in-time.md", &decision("BODY AT COMMIT A"));
    let commit_a = repo.commit_all("fixture: commit A");

    repo.configure_corpus_source("tests/point-in-time-b");
    repo.write("decisions/point-in-time.md", &decision("BODY AT COMMIT B"));
    let commit_b = repo.commit_all("fixture: commit B");
    let status_before = repo.status();

    let at_a = repo.run(&["export", "decisions", "--documents", "--at", &commit_a]);
    let at_a_again = repo.run(&["export", "decisions", "--documents", "--at", &commit_a]);
    let at_b = repo.run(&["export", "decisions", "--documents", "--at", &commit_b]);
    let at_b_again = repo.run(&["export", "decisions", "--documents", "--at", &commit_b]);

    for (output, context) in [
        (&at_a, "documents export at A"),
        (&at_a_again, "repeated documents export at A"),
        (&at_b, "documents export at B"),
        (&at_b_again, "repeated documents export at B"),
    ] {
        assert_success(output, context);
    }
    assert_eq!(at_a.stdout, at_a_again.stdout);
    assert_eq!(at_b.stdout, at_b_again.stdout);
    assert_ne!(at_a.stdout, at_b.stdout);

    let document_a: Value = serde_json::from_slice(&at_a.stdout).expect("documents JSONL at A");
    let document_b: Value = serde_json::from_slice(&at_b.stdout).expect("documents JSONL at B");
    assert!(document_a["text"]
        .as_str()
        .expect("document A text")
        .contains("BODY AT COMMIT A"));
    assert!(document_b["text"]
        .as_str()
        .expect("document B text")
        .contains("BODY AT COMMIT B"));
    assert_eq!(
        document_a["metadata"]["source"], "tests/point-in-time-a",
        "commit A must use commit A's corpus source"
    );
    assert_eq!(
        document_b["metadata"]["source"], "tests/point-in-time-b",
        "commit B must use commit B's corpus source"
    );
    assert_eq!(
        repo.status(),
        status_before,
        "export must not mutate git state"
    );
}

#[test]
fn at_head_is_byte_identical_to_plain_viewer_documents_and_graph_exports() {
    let repo = TestRepo::new("head-parity");
    repo.configure_corpus();
    repo.write("decisions/point-in-time.md", &decision("HEAD PARITY BODY"));
    repo.commit_all("fixture: HEAD parity");

    let absolute_decisions = repo.path("decisions").to_string_lossy().into_owned();
    for directory in [".", "decisions", absolute_decisions.as_str()] {
        for mode in [None, Some("--documents"), Some("--graph")] {
            let mut plain_args = vec!["export", directory];
            if let Some(mode) = mode {
                plain_args.push(mode);
            }
            let plain = repo.run(&plain_args);

            let mut historical_args = plain_args;
            historical_args.push("--at=HEAD");
            let historical = repo.run(&historical_args);

            assert_success(&plain, &format!("plain {mode:?} export from {directory}"));
            assert_success(
                &historical,
                &format!("historical {mode:?} export from {directory}"),
            );
            assert_eq!(
                historical.stdout, plain.stdout,
                "--at=HEAD changed the {mode:?} payload from {directory}"
            );
            assert_eq!(historical.stderr, plain.stderr);
        }
    }
}

#[test]
fn unknown_revision_and_non_git_directory_are_actionable_usage_errors() {
    let repo = TestRepo::new("errors");
    repo.configure_corpus();
    repo.write("decisions/point-in-time.md", &decision("ERROR FIXTURE"));
    repo.commit_all("fixture: error surfaces");

    let unknown_name = "definitely-not-a-revision";
    let unknown = repo.run(&["export", "decisions", "--documents", "--at", unknown_name]);
    assert_eq!(unknown.status.code(), Some(2));
    let unknown_stderr = String::from_utf8_lossy(&unknown.stderr);
    assert!(
        unknown_stderr.contains(&format!("unknown revision: {unknown_name}")),
        "stderr={unknown_stderr}"
    );

    let non_git = std::env::temp_dir().join(format!("asdecided-at-non-git-{}", scratch_suffix()));
    fs::create_dir_all(&non_git).expect("create non-git directory");
    fs::write(non_git.join("README.md"), "not a repository\n").expect("write non-git fixture");
    let non_git_text = non_git.to_string_lossy().into_owned();
    let output = Command::new(env!("CARGO_BIN_EXE_decided"))
        .args(["export", &non_git_text, "--at", "HEAD"])
        .current_dir(&non_git)
        .output()
        .expect("run decided outside git");
    assert_eq!(output.status.code(), Some(2));
    let non_git_stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        non_git_stderr.contains("not a git repository:"),
        "stderr={non_git_stderr}"
    );
    assert!(
        non_git_stderr.contains(&non_git_text),
        "stderr={non_git_stderr}"
    );
    fs::remove_dir_all(non_git).expect("remove non-git fixture");
}

#[test]
fn an_absent_historical_corpus_exports_as_an_empty_valid_payload() {
    let repo = TestRepo::new("absent-corpus");
    repo.configure_corpus();
    repo.write("README.md", "The corpus is added later.\n");
    let before_corpus = repo.commit_all("fixture: before corpus adoption");

    repo.write("decisions/point-in-time.md", &decision("ADOPTED LATER"));
    repo.commit_all("fixture: adopt corpus");

    let output = repo.run(&["export", "decisions", "--at", &before_corpus]);
    assert_success(&output, "empty historical viewer export");
    let payload: Value = serde_json::from_slice(&output.stdout).expect("empty viewer JSON");
    assert_eq!(payload["corpus"]["name"], "decisions");
    assert_eq!(payload["corpus"]["source"], "tests/point-in-time");
    assert_eq!(payload["corpus"]["artifact_count"], 0);
    assert_eq!(payload["artifacts"], Value::Array(Vec::new()));
    assert_eq!(payload["relationships"], Value::Array(Vec::new()));
}

#[test]
fn historical_corpus_can_be_exported_after_its_live_path_is_deleted() {
    let repo = TestRepo::new("deleted-live-path");
    repo.configure_corpus();
    repo.write(
        "decisions/point-in-time.md",
        &decision("HISTORICAL PATH BODY"),
    );
    let historical = repo.commit_all("fixture: corpus path exists");

    fs::remove_dir_all(repo.path("decisions")).expect("remove live corpus path");
    repo.commit_all("fixture: remove live corpus path");

    let output = repo.run(&["export", "decisions", "--documents", "--at", &historical]);
    assert_success(&output, "export deleted live corpus path from history");
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("HISTORICAL PATH BODY"),
        "historical path was resolved through the current worktree: {}",
        String::from_utf8_lossy(&output.stdout)
    );
}

#[cfg(unix)]
#[test]
fn live_symlink_does_not_redirect_the_historical_repository_path() {
    use std::os::unix::fs::symlink;

    let repo = TestRepo::new("live-symlink-path");
    repo.configure_corpus();
    repo.write(
        "decisions/point-in-time.md",
        &decision("ORIGINAL DECISIONS BODY"),
    );
    let historical = repo.commit_all("fixture: original decisions path");

    fs::rename(repo.path("decisions"), repo.path("archive")).expect("rename live corpus");
    repo.write(
        "archive/point-in-time.md",
        &decision("CURRENT ARCHIVE BODY"),
    );
    symlink("archive", repo.path("decisions")).expect("redirect live decisions path");
    repo.commit_all("fixture: redirect decisions through symlink");

    let output = repo.run(&["export", "decisions", "--documents", "--at", &historical]);
    assert_success(
        &output,
        "export lexical historical path through live symlink",
    );
    let body = String::from_utf8_lossy(&output.stdout);
    assert!(body.contains("ORIGINAL DECISIONS BODY"), "stdout={body}");
    assert!(!body.contains("CURRENT ARCHIVE BODY"), "stdout={body}");
}

#[cfg(unix)]
#[test]
fn external_live_symlink_cannot_switch_the_historical_repository() {
    use std::os::unix::fs::symlink;

    let repo = TestRepo::new("external-symlink-source");
    repo.configure_corpus();
    repo.write(
        "alias/point-in-time.md",
        &decision("SOURCE REPOSITORY BODY"),
    );
    let historical = repo.commit_all("fixture: source repository corpus");

    let other = TestRepo::new("external-symlink-target");
    other.configure_corpus_source("tests/other-repository");
    other.write(
        "decisions/point-in-time.md",
        &decision("OTHER REPOSITORY BODY"),
    );
    other.commit_all("fixture: other repository corpus");

    fs::remove_dir_all(repo.path("alias")).expect("remove live source corpus");
    symlink(other.path("decisions"), repo.path("alias"))
        .expect("redirect live corpus to another repository");

    let requested = repo.path("alias").to_string_lossy().into_owned();
    let output = repo.run(&["export", &requested, "--documents", "--at", &historical]);
    assert_success(
        &output,
        "export lexical historical path despite external live symlink",
    );
    let body = String::from_utf8_lossy(&output.stdout);
    assert!(body.contains("SOURCE REPOSITORY BODY"), "stdout={body}");
    assert!(!body.contains("OTHER REPOSITORY BODY"), "stdout={body}");
}

#[cfg(unix)]
#[test]
fn symlinked_checkout_root_remains_a_valid_historical_repository_address() {
    use std::os::unix::fs::symlink;

    let repo = TestRepo::new("symlinked-checkout-root");
    repo.configure_corpus();
    repo.write(
        "decisions/point-in-time.md",
        &decision("SYMLINKED CHECKOUT BODY"),
    );
    repo.commit_all("fixture: symlinked checkout corpus");

    let checkout_link = repo.runtime.join("checkout-link");
    symlink(&repo.root, &checkout_link).expect("create checkout-root symlink");
    let requested = checkout_link
        .join("decisions")
        .to_string_lossy()
        .into_owned();
    let output = repo.run(&["export", &requested, "--documents", "--at", "HEAD"]);
    assert_success(&output, "export through symlinked checkout root");
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("SYMLINKED CHECKOUT BODY"),
        "stdout={}",
        String::from_utf8_lossy(&output.stdout)
    );
}

#[test]
fn at_rejects_write_and_schema_modes_before_writing_and_requires_a_value() {
    let repo = TestRepo::new("usage");
    repo.configure_corpus();
    repo.write("decisions/point-in-time.md", &decision("USAGE FIXTURE"));
    repo.commit_all("fixture: usage guards");
    let worktree_before = repo.worktree_snapshot();
    let status_before = repo.status();

    for args in [
        vec!["export", "decisions", "--html", "--at", "HEAD"],
        vec!["export", "decisions", "--okf", "--at", "HEAD"],
        vec!["export", "decisions", "--agent-rules", "--at", "HEAD"],
        vec!["export", "decisions", "--schema", "viewer", "--at", "HEAD"],
    ] {
        let output = repo.run(&args);
        assert_eq!(output.status.code(), Some(2), "args={args:?}");
        assert!(
            String::from_utf8_lossy(&output.stderr)
                .contains("--at is available only for viewer, documents, and graph exports"),
            "args={args:?}, stderr={}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    for args in [
        vec!["export", "decisions", "--at"],
        vec!["export", "decisions", "--at="],
    ] {
        let output = repo.run(&args);
        assert_eq!(output.status.code(), Some(2), "args={args:?}");
        assert!(
            String::from_utf8_lossy(&output.stderr)
                .contains("argument --at: expected one argument"),
            "args={args:?}, stderr={}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    assert_eq!(
        repo.worktree_snapshot(),
        worktree_before,
        "rejected --at modes wrote into the worktree"
    );
    assert_eq!(
        repo.status(),
        status_before,
        "usage errors mutated git state"
    );
    assert!(!repo.path("lore-export.html").exists());
    assert!(!repo.path("okf-bundle").exists());
    assert!(!repo.path("AGENTS.md").exists());
}
