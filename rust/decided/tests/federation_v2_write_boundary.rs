use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

static COUNTER: AtomicUsize = AtomicUsize::new(0);
const V2_ZERO_DIGEST: &str =
    "sha256-v2:0000000000000000000000000000000000000000000000000000000000000000";

fn scratch(name: &str) -> PathBuf {
    let sequence = COUNTER.fetch_add(1, Ordering::SeqCst);
    let root = std::env::temp_dir().join(format!(
        "asdecided-v2-write-boundary-{name}-{}-{sequence}",
        std::process::id()
    ));
    fs::create_dir_all(root.join(".decided")).unwrap();
    fs::create_dir_all(root.join("decisions")).unwrap();
    fs::write(
        root.join(".decided/config.yaml"),
        "repository_key: APP\ncorpus:\n  source: acme/app\n",
    )
    .unwrap();
    root
}

fn parent(root: &Path, source: &str) {
    fs::create_dir_all(root.join(".decided")).unwrap();
    fs::create_dir_all(root.join("decisions")).unwrap();
    fs::write(
        root.join(".decided/config.yaml"),
        format!("repository_key: STD\ncorpus:\n  source: {source}\n"),
    )
    .unwrap();
}

fn activate_v2(root: &Path) {
    let parents = [
        ("one", "acme/one", "vendor/one"),
        ("two", "acme/two", "vendor/two"),
        ("three", "acme/three", "vendor/three"),
    ];
    for (_, source, path) in parents {
        parent(&root.join(path), source);
    }
    parent(&root.join("vendor/one/vendor/leaf"), "acme/leaf");

    let mut manifest = String::from("# Corpus\n\n## inherits\n\n```yaml\nversion: 2\nparents:\n");
    for (alias, source, path) in parents {
        manifest.push_str(&format!(
            "  - alias: {alias}\n    source: {source}\n    root: {path}\n    corpus: decisions\n    digest: {V2_ZERO_DIGEST}\n"
        ));
    }
    manifest.push_str("```\n\n## overrides\n\n```yaml\nversion: 2\nitems: []\n```\n");
    fs::write(root.join(".decided/corpus.md"), manifest).unwrap();
}

fn run(root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_decided"))
        .args(args)
        .current_dir(root)
        .env("DECIDED_NO_CACHE", "1")
        .output()
        .unwrap()
}

fn assert_refused(output: &Output, context: &str) {
    assert_eq!(output.status.code(), Some(1), "{context}");
    let stderr = String::from_utf8_lossy(&output.stderr).to_lowercase();
    assert!(
        stderr.contains("refusing to write") && stderr.contains("read-only"),
        "{context}: {stderr}"
    );
}

#[test]
fn mutation_and_output_commands_refuse_all_v2_materialisation_routes() {
    let root = scratch("commands");
    activate_v2(&root);
    let direct_mutation = root.join("vendor/two/decisions/forbidden.md");
    let transitive_mutation = root.join("vendor/one/vendor/leaf/decisions/forbidden.md");
    let output = root.join("vendor/three/forbidden.html");

    assert_refused(
        &run(
            &root,
            &[
                "new",
                "requirement",
                "vendor/two/decisions/forbidden.md",
                "--json",
            ],
        ),
        "direct inherited mutation",
    );
    assert_refused(
        &run(
            &root,
            &[
                "new",
                "requirement",
                "vendor/one/vendor/leaf/decisions/forbidden.md",
                "--json",
            ],
        ),
        "transitive inherited mutation",
    );
    assert_refused(
        &run(
            &root,
            &[
                "export",
                "decisions",
                "--html",
                "--out",
                "vendor/three/forbidden.html",
            ],
        ),
        "inherited output",
    );

    assert!(!direct_mutation.exists());
    assert!(!transitive_mutation.exists());
    assert!(!output.exists());
    fs::remove_dir_all(root).unwrap();
}
