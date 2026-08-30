use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::Value;

pub const CHILD_SOURCE: &str = "acme/payments-service";
pub const PARENT_SOURCE: &str = "acme/standards";
pub const PARENT_ALIAS: &str = "standards";
pub const PARENT_DECISION_ID: &str = "STD-KZKMJ8WSMFA1";
pub const PARENT_REQUIREMENT_ID: &str = "STD-KZKMJ92ABVJG";
pub const CHILD_DECISION_ID: &str = "APP-KZKMJ9DGR69Z";
pub const CHILD_REQUIREMENT_ID: &str = "APP-KZKMJ9K3AFB2";
pub const CONSTRAINT_RULE_ID: &str = "parent-forbids-child-marker";
pub const FORBIDDEN_MARKER: &str = "FORBIDDEN_CHILD_MARKER";

const ZERO_DIGEST: &str = "sha256:0000000000000000000000000000000000000000000000000000000000000000";

#[derive(Debug)]
pub struct FederationRepo {
    root: PathBuf,
}

impl FederationRepo {
    pub fn new(label: &str) -> Self {
        assert!(
            label
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte == b'-'),
            "test labels stay filesystem-safe and deterministic"
        );
        let root = std::env::temp_dir().join(format!(
            "asdecided-federation-{label}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join(".decided")).expect("create child config directory");
        fs::create_dir_all(root.join("decisions/decisions"))
            .expect("create child decision directory");
        fs::create_dir_all(root.join("decisions/requirements"))
            .expect("create child requirement directory");
        fs::create_dir_all(root.join("src")).expect("create child source directory");
        fs::create_dir_all(root.join(".git/hooks")).expect("create local hook directory");
        fs::create_dir_all(root.join("vendor/standards/.decided"))
            .expect("create parent config directory");
        fs::create_dir_all(root.join("vendor/standards/decisions/decisions"))
            .expect("create parent decision directory");
        fs::create_dir_all(root.join("vendor/standards/decisions/requirements"))
            .expect("create parent requirement directory");

        let repo = Self { root };
        repo.write(
            ".decided/config.yaml",
            &repository_config("APP", CHILD_SOURCE),
        );
        repo.write(
            "vendor/standards/.decided/config.yaml",
            &repository_config("STD", PARENT_SOURCE),
        );
        repo.write(
            "vendor/standards/decisions/decisions/parent-guardrail.md",
            &decision(PARENT_DECISION_ID, "Parent Guardrail", "Accepted", true),
        );
        repo.write(
            "vendor/standards/decisions/requirements/parent-requirement.md",
            &requirement(
                PARENT_REQUIREMENT_ID,
                "Parent Requirement",
                "The shared standards corpus MUST retain one inherited requirement.",
                Some(PARENT_DECISION_ID),
            ),
        );
        repo.write(
            "decisions/decisions/local-rationale.md",
            &decision(CHILD_DECISION_ID, "Local Rationale", "Accepted", false),
        );
        repo.write(
            "decisions/requirements/local-requirement.md",
            &requirement(
                CHILD_REQUIREMENT_ID,
                "Local Requirement",
                "The child corpus MUST retain one local requirement.",
                Some(CHILD_DECISION_ID),
            ),
        );
        repo.write("src/guarded.rs", "pub fn guarded() {}\n");
        repo
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn parent_root(&self) -> PathBuf {
        self.root.join("vendor/standards")
    }

    pub fn write(&self, relative: &str, contents: &str) {
        let relative = Path::new(relative);
        assert!(!relative.is_absolute(), "fixture write must be relative");
        assert!(
            relative.components().all(|component| matches!(
                component,
                std::path::Component::Normal(_) | std::path::Component::CurDir
            )),
            "fixture write must not contain root or parent components"
        );
        let path = self.root.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create fixture parent directory");
        }
        fs::write(path, contents).expect("write federation fixture");
    }

    pub fn append(&self, relative: &str, contents: &str) {
        use std::io::Write;
        let path = self.root.join(relative);
        let mut file = fs::OpenOptions::new()
            .append(true)
            .open(path)
            .expect("open federation fixture for append");
        file.write_all(contents.as_bytes())
            .expect("append federation fixture");
    }

    pub fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_decided"))
            .args(args)
            .current_dir(&self.root)
            .env("DECIDED_NO_CACHE", "1")
            .env("DECIDED_CACHE_DIR", self.root.join(".decided/cache"))
            .env("XDG_CACHE_HOME", self.root.join(".xdg/cache"))
            .env("XDG_CONFIG_HOME", self.root.join(".xdg/config"))
            .env("XDG_STATE_HOME", self.root.join(".xdg/state"))
            .output()
            .expect("run decided federation command")
    }

    pub fn parent_digest(&self) -> String {
        let output = self.run(&[
            "corpus",
            "digest",
            "--root",
            "vendor/standards",
            "--corpus",
            "decisions",
        ]);
        assert_success(&output, "calculate parent corpus digest");
        let stdout = stdout(&output);
        assert!(
            stdout.ends_with('\n'),
            "digest output needs one newline: {stdout:?}"
        );
        let digest = stdout
            .strip_suffix('\n')
            .expect("digest newline already checked");
        assert_digest(digest);
        assert_eq!(stdout, format!("{digest}\n"));
        digest.to_string()
    }

    pub fn activate(&self) -> String {
        self.activate_with_override(None)
    }

    pub fn activate_with_override(&self, override_yaml: Option<&str>) -> String {
        let digest = self.parent_digest();
        self.write_manifest(
            PARENT_SOURCE,
            "vendor/standards",
            "decisions",
            &digest,
            override_yaml,
        );
        digest
    }

    pub fn write_manifest(
        &self,
        source: &str,
        root: &str,
        corpus: &str,
        digest: &str,
        override_yaml: Option<&str>,
    ) {
        assert_digest(digest);
        let mut manifest = format!(
            "# Corpus\n\n## inherits\n\n```yaml\nversion: 1\nalias: {PARENT_ALIAS}\nsource: {source}\nroot: {}\ncorpus: {}\ndigest: {digest}\n```\n",
            yaml_string(root),
            yaml_string(corpus),
        );
        if let Some(overrides) = override_yaml {
            manifest.push_str("\n## overrides\n\n```yaml\n");
            manifest.push_str(overrides);
            if !overrides.ends_with('\n') {
                manifest.push('\n');
            }
            manifest.push_str("```\n");
        }
        self.write(".decided/corpus.md", &manifest);
    }

    pub fn write_child_requirement_reference(&self, target: &str) {
        self.write(
            "decisions/requirements/local-requirement.md",
            &requirement(
                CHILD_REQUIREMENT_ID,
                "Local Requirement",
                "The child corpus MUST resolve its declared governing decision.",
                Some(target),
            ),
        );
    }

    pub fn write_child_replacement(&self) {
        self.write(
            "decisions/requirements/local-requirement.md",
            &requirement(
                CHILD_REQUIREMENT_ID,
                "Local Replacement",
                "The child corpus MUST use its explicit local replacement.",
                Some(CHILD_DECISION_ID),
            ),
        );
    }

    pub fn write_child_rationale(&self, status: &str) {
        self.write(
            "decisions/decisions/local-rationale.md",
            &decision(CHILD_DECISION_ID, "Local Rationale", status, false),
        );
    }

    pub fn write_child_collision(&self) {
        self.write(
            "decisions/decisions/colliding-decision.md",
            &decision(
                PARENT_DECISION_ID,
                "Colliding Local Decision",
                "Accepted",
                false,
            ),
        );
    }

    pub fn write_parent_manifest(&self) {
        self.write(
            "vendor/standards/.decided/corpus.md",
            &format!(
                "# Corpus\n\n## inherits\n\n```yaml\nversion: 1\nalias: upstream\nsource: acme/upstream\nroot: 'vendor/upstream'\ncorpus: 'decisions'\ndigest: {ZERO_DIGEST}\n```\n"
            ),
        );
    }

    pub fn parent_snapshot(&self) -> TreeSnapshot {
        TreeSnapshot::capture(&self.parent_root())
    }
}

impl Drop for FederationRepo {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

pub fn override_yaml(parent: &str, replacement: &str, rationale: &str) -> String {
    format!(
        "version: 1\nitems:\n  - parent: {parent}\n    with: {replacement}\n    rationale: {rationale}\n"
    )
}

pub fn qualified(id: &str) -> String {
    format!("{PARENT_ALIAS}::{id}")
}

pub fn assert_success(output: &Output, context: &str) {
    assert!(
        output.status.success(),
        "{context} failed with {:?}\nstdout:\n{}\nstderr:\n{}",
        output.status.code(),
        stdout(output),
        stderr(output)
    );
}

pub fn assert_exit(output: &Output, expected: i32, context: &str) {
    assert_eq!(
        output.status.code(),
        Some(expected),
        "{context}\nstdout:\n{}\nstderr:\n{}",
        stdout(output),
        stderr(output)
    );
}

pub fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

pub fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

pub fn combined(output: &Output) -> String {
    format!("{}{}", stdout(output), stderr(output))
}

pub fn parse_json(output: &Output, context: &str) -> Value {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "{context} did not emit JSON: {error}\nstdout:\n{}\nstderr:\n{}",
            stdout(output),
            stderr(output)
        )
    })
}

pub fn parse_json_lines(output: &Output, context: &str) -> Vec<Value> {
    stdout(output)
        .lines()
        .enumerate()
        .map(|(index, line)| {
            serde_json::from_str(line).unwrap_or_else(|error| {
                panic!(
                    "{context} line {} did not emit JSON: {error}\nline:\n{line}\nstderr:\n{}",
                    index + 1,
                    stderr(output)
                )
            })
        })
        .collect()
}

pub fn json_has_string_field(value: &Value, key: &str, expected: &str) -> bool {
    match value {
        Value::Object(map) => {
            map.get(key).and_then(Value::as_str) == Some(expected)
                || map
                    .values()
                    .any(|child| json_has_string_field(child, key, expected))
        }
        Value::Array(items) => items
            .iter()
            .any(|child| json_has_string_field(child, key, expected)),
        _ => false,
    }
}

pub fn assert_inherited_provenance(value: &Value, digest: &str) {
    assert!(
        json_has_string_field(value, "source", PARENT_SOURCE),
        "missing inherited source in {value}"
    );
    assert!(
        json_has_string_field(value, "layer", "inherited"),
        "missing inherited layer in {value}"
    );
    assert!(
        json_has_string_field(value, "pin", digest),
        "missing inherited pin in {value}"
    );
}

pub fn assert_local_provenance(value: &Value) {
    assert!(
        json_has_string_field(value, "source", CHILD_SOURCE),
        "missing local source in {value}"
    );
    assert!(
        json_has_string_field(value, "layer", "local"),
        "missing local layer in {value}"
    );
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct TreeSnapshot(BTreeMap<String, SnapshotEntry>);

#[derive(Debug, Clone, Eq, PartialEq)]
enum SnapshotEntry {
    Directory,
    File(Vec<u8>),
    Symlink(PathBuf),
}

impl TreeSnapshot {
    pub fn capture(root: &Path) -> Self {
        let mut entries = BTreeMap::new();
        capture_tree(root, root, &mut entries);
        Self(entries)
    }
}

fn capture_tree(root: &Path, current: &Path, entries: &mut BTreeMap<String, SnapshotEntry>) {
    let mut children: Vec<PathBuf> = fs::read_dir(current)
        .expect("read snapshot directory")
        .map(|entry| entry.expect("read snapshot entry").path())
        .collect();
    children.sort();
    for path in children {
        let relative = path
            .strip_prefix(root)
            .expect("snapshot path beneath root")
            .to_string_lossy()
            .replace('\\', "/");
        let metadata = fs::symlink_metadata(&path).expect("read snapshot metadata");
        if metadata.file_type().is_symlink() {
            entries.insert(
                relative,
                SnapshotEntry::Symlink(fs::read_link(&path).expect("read snapshot symlink")),
            );
        } else if metadata.is_dir() {
            entries.insert(relative, SnapshotEntry::Directory);
            capture_tree(root, &path, entries);
        } else {
            entries.insert(
                relative,
                SnapshotEntry::File(fs::read(&path).expect("read snapshot file")),
            );
        }
    }
}

fn assert_digest(digest: &str) {
    let hex = digest
        .strip_prefix("sha256:")
        .unwrap_or_else(|| panic!("digest lacks sha256 prefix: {digest:?}"));
    assert_eq!(hex.len(), 64, "digest is not full SHA-256: {digest:?}");
    assert!(
        hex.bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)),
        "digest is not lowercase hexadecimal: {digest:?}"
    );
}

fn yaml_string(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

fn repository_config(key: &str, source: &str) -> String {
    format!("repository_key: {key}\ncorpus:\n  source: {source}\n")
}

fn decision(id: &str, title: &str, status: &str, constrained: bool) -> String {
    let mut body = format!(
        "---\nschema_version: 1\nid: {id}\ntype: decision\n---\n# {title}\n\n## Status\n\n{status}\n\n## Category\n\nTechnical\n\n## Context\n\nThe fixture needs a deterministic governing decision.\n\n## Decision\n\nKeep the federation boundary explicit and testable.\n\n## Consequences\n\nThe black-box suite can identify the governing source.\n"
    );
    if constrained {
        body.push_str(&format!(
            "\n## Applies To\n\n- src/**/*.rs\n\n## Code Constraints\n\n```yaml\nversion: 1\neligibility: eligible\nrules:\n  - id: {CONSTRAINT_RULE_ID}\n    kind: forbid_pattern\n    path_glob: \"src/**/*.rs\"\n    pattern: \"{FORBIDDEN_MARKER}\"\n    message: \"Inherited standards forbid the child marker.\"\n```\n"
        ));
    }
    body
}

fn requirement(id: &str, title: &str, statement: &str, related_decision: Option<&str>) -> String {
    let mut body = format!(
        "---\nschema_version: 1\nid: {id}\ntype: requirement\n---\n# Requirement: {title}\n\n## Status\n\nProposed\n\n## Problem\n\nThe fixture needs deterministic federation coverage.\n\n## Requirements\n\n- [REQ-001] {statement}\n\n## Acceptance Criteria\n\n- The recorded federation behaviour is observable through the public CLI.\n\n## Success Metrics\n\n- The black-box assertion passes deterministically.\n\n## Risks\n\n- A divergent read path could omit inherited governance.\n\n## Assumptions\n\n- The parent is already materialised inside the child repository.\n"
    );
    if let Some(target) = related_decision {
        body.push_str(&format!("\n## Related Decisions\n\n- {target}\n"));
    }
    body
}
