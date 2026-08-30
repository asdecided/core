//! Independent fixture construction for the manifest-v2 black-box contract.
//!
//! This module intentionally does not call the engine's federation APIs. The
//! v2 pins are calculated with the public SHA-256 framing contract so a shared
//! implementation defect cannot make both fixture and engine agree.

#![allow(dead_code)]

use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static SCRATCH_SEQUENCE: AtomicU64 = AtomicU64::new(0);

pub const ROOT_SOURCE: &str = "acme/root";
pub const SHARED_SOURCE: &str = "acme/shared";
pub const SHARED_ID: &str = "SHR-01K000000001";
pub const ROOT_ID: &str = "APP-01K000000001";

#[derive(Clone, Debug)]
pub struct ParentEdge {
    pub alias: String,
    pub source: String,
    pub root: String,
    pub corpus: String,
    pub digest: String,
}

impl ParentEdge {
    pub fn new(alias: &str, source: &str, root: &str, digest: String) -> Self {
        Self {
            alias: alias.to_string(),
            source: source.to_string(),
            root: root.to_string(),
            corpus: "decisions".to_string(),
            digest,
        }
    }
}

#[derive(Clone, Debug)]
pub struct OverrideEdge {
    pub target: String,
    pub replacement: String,
    pub rationale: String,
}

impl OverrideEdge {
    pub fn new(target: &str, replacement: &str, rationale: &str) -> Self {
        Self {
            target: target.to_string(),
            replacement: replacement.to_string(),
            rationale: rationale.to_string(),
        }
    }
}

#[derive(Debug)]
pub struct GraphRepo {
    root: PathBuf,
}

impl GraphRepo {
    pub fn new(label: &str) -> Self {
        assert!(
            label
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte == b'-'),
            "fixture labels must stay portable"
        );
        let sequence = SCRATCH_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock after epoch")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "asdecided-federation-v2-{label}-{}-{nonce}-{sequence}",
            std::process::id()
        ));
        fs::create_dir_all(&root).expect("create graph fixture root");
        let repo = Self { root };
        repo.create_node("", "APP", ROOT_SOURCE);
        repo.write("decisions/root.md", &decision(ROOT_ID, "Root Policy", None));
        repo.write("src/guarded.rs", "pub fn guarded() {}\n");
        repo
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn path(&self, relative: &str) -> PathBuf {
        if relative.is_empty() {
            self.root.clone()
        } else {
            self.root.join(relative)
        }
    }

    pub fn create_node(&self, relative: &str, key: &str, source: &str) {
        let node = self.path(relative);
        fs::create_dir_all(node.join(".decided")).expect("create node config directory");
        fs::create_dir_all(node.join("decisions")).expect("create node corpus");
        fs::write(
            node.join(".decided/config.yaml"),
            format!("repository_key: {key}\ncorpus:\n  source: {source}\n"),
        )
        .expect("write node config");
    }

    pub fn write(&self, relative: &str, contents: &str) {
        let relative = Path::new(relative);
        assert!(!relative.is_absolute(), "fixture write must be relative");
        assert!(
            relative.components().all(|component| matches!(
                component,
                std::path::Component::Normal(_) | std::path::Component::CurDir
            )),
            "fixture write cannot escape its root"
        );
        let path = self.root.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create fixture directory");
        }
        fs::write(path, contents).expect("write fixture file");
    }

    pub fn write_node(&self, node: &str, relative: &str, contents: &str) {
        let path = if node.is_empty() {
            relative.to_string()
        } else {
            format!("{node}/{relative}")
        };
        self.write(&path, contents);
    }

    pub fn write_v2_manifest(
        &self,
        node: &str,
        parents: &[ParentEdge],
        overrides: &[OverrideEdge],
    ) {
        assert!(!parents.is_empty(), "v2 manifest needs at least one parent");
        let mut manifest =
            String::from("# Corpus\n\n## inherits\n\n```yaml\nversion: 2\nparents:\n");
        for parent in parents {
            manifest.push_str(&format!(
                "  - alias: {}\n    source: {}\n    root: '{}'\n    corpus: '{}'\n    digest: {}\n",
                parent.alias, parent.source, parent.root, parent.corpus, parent.digest
            ));
        }
        manifest.push_str("```\n");
        if !overrides.is_empty() {
            manifest.push_str("\n## overrides\n\n```yaml\nversion: 2\nitems:\n");
            for mapping in overrides {
                manifest.push_str(&format!(
                    "  - target: {}\n    with: {}\n    rationale: {}\n",
                    mapping.target, mapping.replacement, mapping.rationale
                ));
            }
            manifest.push_str("```\n");
        }
        self.write_node(node, ".decided/corpus.md", &manifest);
    }

    pub fn v2_digest(&self, node: &str, source: &str) -> String {
        calculate_v2_digest(&self.path(node), "decisions", source)
    }

    pub fn copy_node(&self, source: &str, target: &str) {
        copy_tree(&self.path(source), &self.path(target));
    }
}

impl Drop for GraphRepo {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

pub fn calculate_v2_digest(root: &Path, corpus: &str, source: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"asdecided-corpus-digest-v2\0");
    frame(&mut hasher, 0x01, source.as_bytes());
    frame(
        &mut hasher,
        0x02,
        &fs::read(root.join(".decided/config.yaml")).expect("read digest config"),
    );
    let manifest = root.join(".decided/corpus.md");
    if manifest.is_file() {
        frame(&mut hasher, 0x03, &[1]);
        frame(
            &mut hasher,
            0x04,
            &fs::read(manifest).expect("read digest manifest"),
        );
    } else {
        frame(&mut hasher, 0x03, &[0]);
    }

    let corpus_root = root.join(corpus);
    let mut files = Vec::new();
    collect_markdown(&corpus_root, &corpus_root, &mut files);
    files.sort_by(|left, right| left.0.as_bytes().cmp(right.0.as_bytes()));
    for (relative, bytes) in files {
        frame(&mut hasher, 0x05, relative.as_bytes());
        frame(&mut hasher, 0x06, &bytes);
    }
    format!("sha256-v2:{:x}", hasher.finalize())
}

fn frame(hasher: &mut Sha256, tag: u8, payload: &[u8]) {
    hasher.update([tag]);
    hasher.update((payload.len() as u64).to_be_bytes());
    hasher.update(payload);
}

fn collect_markdown(root: &Path, current: &Path, files: &mut Vec<(String, Vec<u8>)>) {
    let mut entries = fs::read_dir(current)
        .unwrap_or_else(|error| panic!("read {}: {error}", current.display()))
        .map(|entry| entry.expect("read corpus entry").path())
        .collect::<Vec<_>>();
    entries.sort();
    for path in entries {
        let metadata = fs::symlink_metadata(&path).expect("read corpus metadata");
        if metadata.is_dir() {
            collect_markdown(root, &path, files);
        } else if metadata.is_file()
            && path
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("md"))
        {
            let relative = path
                .strip_prefix(root)
                .expect("corpus file beneath root")
                .to_string_lossy()
                .replace('\\', "/");
            files.push((relative, fs::read(path).expect("read corpus file")));
        }
    }
}

fn copy_tree(source: &Path, target: &Path) {
    fs::create_dir_all(target).expect("create copied node");
    for entry in fs::read_dir(source).expect("read copied node") {
        let entry = entry.expect("read copied entry");
        let destination = target.join(entry.file_name());
        let file_type = entry.file_type().expect("copied entry type");
        if file_type.is_dir() {
            copy_tree(&entry.path(), &destination);
        } else {
            fs::copy(entry.path(), destination).expect("copy node file");
        }
    }
}

pub fn decision(id: &str, title: &str, related: Option<&str>) -> String {
    let mut body = format!(
        "---\nschema_version: 1\nid: {id}\ntype: decision\n---\n# {title}\n\n## Status\n\nAccepted\n\n## Category\n\nTechnical\n\n## Context\n\nThe graph fixture needs a reviewed policy.\n\n## Decision\n\nKeep source-aware graph behaviour deterministic.\n\n## Consequences\n\nThe public contract remains testable.\n"
    );
    if let Some(target) = related {
        body.push_str(&format!("\n## Related Decisions\n\n- {target}\n"));
    }
    body
}

pub fn constrained_decision(id: &str, title: &str, marker: &str) -> String {
    format!(
        "{}\n## Applies To\n\n- src/**/*.rs\n\n## Code Constraints\n\n```yaml\nversion: 1\neligibility: eligible\nrules:\n  - id: inherited-v2-guard\n    kind: forbid_pattern\n    path_glob: \"src/**/*.rs\"\n    pattern: \"{marker}\"\n    message: \"Inherited graph policy forbids the marker.\"\n```\n",
        decision(id, title, None)
    )
}

pub fn requirement(id: &str, title: &str, related: Option<&str>) -> String {
    let mut body = format!(
        "---\nschema_version: 1\nid: {id}\ntype: requirement\n---\n# Requirement: {title}\n\n## Status\n\nProposed\n\n## Problem\n\nThe graph fixture needs a stable requirement.\n\n## Requirements\n\n- [REQ-001] The composed corpus MUST keep this requirement attributable.\n\n## Acceptance Criteria\n\n- Public reads expose the source-aware record.\n\n## Success Metrics\n\n- The contract assertion passes.\n\n## Risks\n\n- An independent overlay could lose provenance.\n\n## Assumptions\n\n- Every inherited source is already materialised.\n"
    );
    if let Some(target) = related {
        body.push_str(&format!("\n## Related Decisions\n\n- {target}\n"));
    }
    body
}
