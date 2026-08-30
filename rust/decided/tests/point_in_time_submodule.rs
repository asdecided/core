//! Black-box coverage for historical federation through a local Git submodule.

mod federation_graph_support;

use federation_graph_support::{decision, GraphRepo, ParentEdge};
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const PARENT_ID: &str = "STD-01K000000001";
const PARENT_SOURCE: &str = "acme/standards";
const P1_TITLE: &str = "P1 Standards Policy";
const P2_TITLE: &str = "P2 Standards Policy";

struct ParentRepo {
    root: PathBuf,
}

impl ParentRepo {
    fn new(superproject: &GraphRepo) -> Self {
        let name = superproject
            .root()
            .file_name()
            .expect("superproject has a final path component")
            .to_string_lossy();
        let root = superproject
            .root()
            .with_file_name(format!("{name}-submodule-origin"));
        fs::create_dir_all(root.join(".decided")).expect("create parent config directory");
        fs::create_dir_all(root.join("decisions")).expect("create parent corpus");
        fs::write(
            root.join(".decided/config.yaml"),
            format!("repository_key: STD\ncorpus:\n  source: {PARENT_SOURCE}\n"),
        )
        .expect("write parent config");

        let parent = Self { root };
        assert_success(
            &git(&parent.root, &["init", "--quiet"]),
            "initialize parent repository",
        );
        assert_success(
            &git(&parent.root, &["config", "user.name", "AsDecided Test"]),
            "configure parent author name",
        );
        assert_success(
            &git(
                &parent.root,
                &["config", "user.email", "tests@asdecided.dev"],
            ),
            "configure parent author email",
        );
        assert_success(
            &git(&parent.root, &["config", "core.autocrlf", "false"]),
            "disable parent line-ending conversion",
        );
        parent.write_policy(P1_TITLE);
        parent.commit("test: parent P1");
        parent
    }

    fn write_policy(&self, title: &str) {
        fs::write(
            self.root.join("decisions/standards.md"),
            decision(PARENT_ID, title, None),
        )
        .expect("write parent policy");
    }

    fn commit(&self, message: &str) -> String {
        assert_success(&git(&self.root, &["add", "--all"]), "stage parent revision");
        assert_success(
            &git(&self.root, &["commit", "--quiet", "--message", message]),
            "commit parent revision",
        );
        git_stdout(
            &self.root,
            &["rev-parse", "HEAD"],
            "resolve parent revision",
        )
    }
}

impl Drop for ParentRepo {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn git(directory: &Path, args: &[&str]) -> Output {
    Command::new("git")
        .args(args)
        .current_dir(directory)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .expect("run Git for submodule fixture")
}

fn git_stdout(directory: &Path, args: &[&str], context: &str) -> String {
    let output = git(directory, args);
    assert_success(&output, context);
    String::from_utf8(output.stdout)
        .expect("Git output is UTF-8")
        .trim()
        .to_string()
}

fn run(repo: &GraphRepo, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_decided"))
        .args(args)
        .current_dir(repo.root())
        .env("DECIDED_CACHE_DIR", repo.root().join(".decided/cache"))
        .env("XDG_CACHE_HOME", repo.root().join(".xdg/cache"))
        .env("XDG_CONFIG_HOME", repo.root().join(".xdg/config"))
        .env("XDG_STATE_HOME", repo.root().join(".xdg/state"))
        .output()
        .expect("run decided submodule federation contract")
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

fn commit_superproject(repo: &GraphRepo, message: &str) -> String {
    assert_success(&git(repo.root(), &["add", "--all"]), "stage superproject");
    assert_success(
        &git(
            repo.root(),
            &[
                "-c",
                "user.name=AsDecided Test",
                "-c",
                "user.email=tests@asdecided.dev",
                "commit",
                "--quiet",
                "--message",
                message,
            ],
        ),
        "commit superproject",
    );
    git_stdout(
        repo.root(),
        &["rev-parse", "HEAD"],
        "resolve superproject revision",
    )
}

fn documents(output: &Output, context: &str) -> Vec<Value> {
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|line| {
            serde_json::from_str(line)
                .unwrap_or_else(|error| panic!("{context} emitted invalid JSONL: {error}: {line}"))
        })
        .collect()
}

#[test]
fn point_in_time_export_follows_recorded_submodule_commit_not_its_worktree() {
    let repo = GraphRepo::new("point-in-time-submodule");
    let parent = ParentRepo::new(&repo);
    let p1 = git_stdout(&parent.root, &["rev-parse", "HEAD"], "resolve parent P1");

    assert_success(
        &git(repo.root(), &["init", "--quiet"]),
        "initialize superproject",
    );
    let parent_url = parent.root.to_string_lossy().into_owned();
    assert_success(
        &git(
            repo.root(),
            &[
                "-c",
                "protocol.file.allow=always",
                "submodule",
                "add",
                "--quiet",
                &parent_url,
                "vendor/standards",
            ],
        ),
        "add local parent as a Git submodule",
    );
    let pin = repo.v2_digest("vendor/standards", PARENT_SOURCE);
    repo.write_v2_manifest(
        "",
        &[ParentEdge::new(
            "standards",
            PARENT_SOURCE,
            "vendor/standards",
            pin.clone(),
        )],
        &[],
    );
    let super_revision = commit_superproject(&repo, "test: pin parent submodule at P1");

    let recorded_gitlink = git_stdout(
        repo.root(),
        &["ls-tree", "HEAD", "vendor/standards"],
        "read recorded submodule gitlink",
    );
    assert!(
        recorded_gitlink.contains(&format!("commit {p1}\tvendor/standards")),
        "superproject did not record P1 as its gitlink: {recorded_gitlink}"
    );

    let plain = run(&repo, &["export", "decisions", "--documents"]);
    assert_success(&plain, "export checked-out submodule federation");
    let historical = run(
        &repo,
        &[
            "export",
            "decisions",
            "--documents",
            "--at",
            &super_revision,
        ],
    );
    assert_success(&historical, "export historical submodule federation");
    assert_eq!(
        historical.stdout, plain.stdout,
        "historical submodule export differs from the matching checkout"
    );
    assert_eq!(historical.stderr, plain.stderr);

    let inherited = documents(&historical, "historical submodule export")
        .into_iter()
        .find(|document| document["id"].as_str() == Some(PARENT_ID))
        .expect("historical export includes the inherited submodule policy");
    assert!(
        inherited["text"]
            .as_str()
            .is_some_and(|text| text.contains(P1_TITLE)),
        "historical export omitted P1 content: {inherited}"
    );
    assert_eq!(
        inherited["metadata"]["source"].as_str(),
        Some(PARENT_SOURCE)
    );
    assert_eq!(
        inherited["metadata"]["provenance"]["source"].as_str(),
        Some(PARENT_SOURCE)
    );
    assert_eq!(
        inherited["metadata"]["provenance"]["layer"].as_str(),
        Some("inherited")
    );
    assert_eq!(
        inherited["metadata"]["provenance"]["pin"].as_str(),
        Some(pin.as_str())
    );

    parent.write_policy(P2_TITLE);
    let p2 = parent.commit("test: parent P2");
    assert_ne!(p2, p1, "parent P2 must advance beyond P1");
    let submodule = repo.path("vendor/standards");
    assert_success(
        &git(
            &submodule,
            &[
                "-c",
                "protocol.file.allow=always",
                "fetch",
                "--quiet",
                "origin",
            ],
        ),
        "fetch P2 into the local submodule clone",
    );
    assert_success(
        &git(&submodule, &["checkout", "--quiet", &p2]),
        "advance the submodule worktree to P2",
    );
    assert_eq!(
        git_stdout(
            &submodule,
            &["rev-parse", "HEAD"],
            "read submodule worktree revision"
        ),
        p2,
        "submodule worktree did not advance to P2"
    );
    assert_eq!(
        git_stdout(
            repo.root(),
            &["rev-parse", "HEAD"],
            "re-read superproject revision"
        ),
        super_revision,
        "advancing the submodule worktree changed the superproject commit"
    );

    let historical_after_p2 = run(
        &repo,
        &[
            "export",
            "decisions",
            "--documents",
            "--at",
            &super_revision,
        ],
    );
    assert_success(
        &historical_after_p2,
        "re-export historical submodule federation after advancing its worktree",
    );
    assert_eq!(
        historical_after_p2.stdout, historical.stdout,
        "ambient submodule worktree state changed a point-in-time export"
    );
    assert_eq!(historical_after_p2.stderr, historical.stderr);
    let rendered = String::from_utf8_lossy(&historical_after_p2.stdout);
    assert!(
        rendered.contains(P1_TITLE),
        "historical export lost P1: {rendered}"
    );
    assert!(
        !rendered.contains(P2_TITLE),
        "historical export leaked P2 worktree bytes: {rendered}"
    );
}
