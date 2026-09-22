//! A long-running `decided-mcp` follows the artifact-type registry (ADR-083,
//! ADR-150): a re-pin or a removed stanza lands on the next request with the
//! cache on, and a version-1 parent bundle edited without a re-pin fails the
//! next request on the warm path, as it does with the cache off.

use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use serde_json::{json, Value};

fn scratch(tag: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "decided-mcp-registry-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&root).unwrap();
    root
}

fn copy_tree(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).unwrap();
        }
    }
}

fn fixture(relative: &str, tag: &str) -> PathBuf {
    let root = scratch(tag);
    copy_tree(
        &Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../fixtures")
            .join(relative),
        &root,
    );
    root
}

struct Server {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next: u64,
}

impl Server {
    /// A server with the persistent cache on (the default), isolated in its
    /// own cache directory.
    fn start(corpus: &Path, cache: &Path) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_decided-mcp"))
            .arg("--root")
            .arg(corpus)
            .env_remove("DECIDED_NO_CACHE")
            .env("DECIDED_CACHE_DIR", cache)
            .env("LC_ALL", "C")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn decided-mcp");
        let stdin = child.stdin.take().unwrap();
        let stdout = BufReader::new(child.stdout.take().unwrap());
        let mut server = Self {
            child,
            stdin,
            stdout,
            next: 1,
        };
        server.send(
            "initialize",
            json!({"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"t","version":"1"}}),
        );
        server
    }

    fn send(&mut self, method: &str, params: Value) -> Value {
        let frame = json!({"jsonrpc": "2.0", "id": self.next, "method": method, "params": params});
        self.next += 1;
        writeln!(self.stdin, "{frame}").unwrap();
        self.stdin.flush().unwrap();
        let mut line = String::new();
        self.stdout.read_line(&mut line).unwrap();
        serde_json::from_str(line.trim()).unwrap()
    }

    fn summary(&mut self) -> Result<Value, String> {
        let frame = self.send(
            "tools/call",
            json!({"name": "get_summary", "arguments": {}}),
        );
        let text = frame["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .to_string();
        if frame["result"]["isError"] == true {
            Err(text)
        } else {
            Ok(serde_json::from_str::<Value>(&text).unwrap()["artifacts"]["by_type"].clone())
        }
    }

    fn finish(mut self) {
        drop(self.stdin);
        let status = self.child.wait().unwrap();
        assert!(status.success());
    }
}

fn sha256(bytes: &[u8]) -> String {
    rac_engine::sha256::hexdigest(bytes)
}

/// Rewrite `config`'s pinned digest to the digest of `bundle`'s bytes.
fn repin(config: &Path, bundle: &[u8]) {
    let text = fs::read_to_string(config).unwrap();
    let start = text.find("digest: sha256:").unwrap();
    let end = start + "digest: sha256:".len() + 64;
    fs::write(
        config,
        format!(
            "{}digest: sha256:{}{}",
            &text[..start],
            sha256(bundle),
            &text[end..]
        ),
    )
    .unwrap();
}

#[test]
fn a_repin_and_a_removed_stanza_land_on_the_next_cached_request() {
    let root = fixture("spec-bundle", "unfederated");
    let cache = scratch("unfederated-cache");
    let mut server = Server::start(&root.join("decisions"), &cache);
    let first = server.summary().unwrap();
    assert_eq!(first["policy"], 1);
    assert_eq!(first["runbook"], 1);

    // Re-pin a bundle without `policy`: the policy is untyped next request.
    let bundle_path = root.join(".decided/artifact-specs.json");
    let mut bundle: Value = serde_json::from_slice(&fs::read(&bundle_path).unwrap()).unwrap();
    bundle["artifact_specs"]
        .as_array_mut()
        .unwrap()
        .retain(|element| element["name"] != "policy");
    let bytes = format!("{}\n", serde_json::to_string_pretty(&bundle).unwrap());
    fs::write(&bundle_path, &bytes).unwrap();
    repin(&root.join(".decided/config.yaml"), bytes.as_bytes());
    let repinned = server.summary().unwrap();
    assert!(repinned.get("policy").is_none(), "{repinned}");
    assert_eq!(repinned["runbook"], 1);
    assert_eq!(repinned["unknown"], 1);

    // Remove the stanza: the built-ins alone, next request.
    let config = root.join(".decided/config.yaml");
    let text = fs::read_to_string(&config).unwrap();
    fs::write(&config, &text[..text.find("# ADR-083").unwrap()]).unwrap();
    let plain = server.summary().unwrap();
    assert!(plain.get("runbook").is_none(), "{plain}");
    assert_eq!(plain["unknown"], 2);
    server.finish();
}

#[test]
fn a_version_one_parent_bundle_edited_without_a_repin_fails_the_warm_path() {
    let root = fixture("eval/federation/child", "v1-tamper");
    let parent = root.join("vendor/standards");
    let bundle = fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../fixtures/spec-federation/vendor/standards/.decided/artifact-specs.json"),
    )
    .unwrap();
    fs::write(parent.join(".decided/artifact-specs.json"), &bundle).unwrap();
    let mut config = fs::read_to_string(parent.join(".decided/config.yaml")).unwrap();
    config.push_str(&format!(
        "artifact_types:\n  version: 1\n  bundle:\n    path: .decided/artifact-specs.json\n    digest: sha256:{}\n",
        sha256(&bundle)
    ));
    fs::write(parent.join(".decided/config.yaml"), config).unwrap();
    fs::create_dir_all(parent.join("decisions/runbooks")).unwrap();
    fs::write(
        parent.join("decisions/runbooks/deploy.md"),
        "---\nschema_version: 1\nid: STD-000000000099\ntype: runbook\n---\n# Deploy\n\n## Status\n\nActive\n\n## Purpose\n\nRoll a build.\n\n## Steps\n\n1. Deploy.\n",
    )
    .unwrap();
    let digest = rac_engine::federation::calculate_parent_digest(&parent, "decisions")
        .unwrap()
        .digest;
    let manifest = root.join(".decided/corpus.md");
    let text = fs::read_to_string(&manifest).unwrap();
    let start = text.find("digest: sha256:").unwrap();
    let end = start + "digest: ".len() + "sha256:".len() + 64;
    fs::write(
        &manifest,
        format!("{}digest: {digest}{}", &text[..start], &text[end..]),
    )
    .unwrap();

    let cache = scratch("v1-tamper-cache");
    let mut server = Server::start(&root.join("decisions"), &cache);
    assert_eq!(server.summary().unwrap()["runbook"], 1);
    // The same generation again is served warm.
    assert_eq!(server.summary().unwrap()["runbook"], 1);

    let mut edited = bundle.clone();
    edited.extend_from_slice(b" \n");
    fs::write(parent.join(".decided/artifact-specs.json"), edited).unwrap();
    let error = server.summary().unwrap_err();
    assert!(
        error.starts_with("artifact-spec-bundle-digest-mismatch: inherited source "),
        "{error}"
    );
    server.finish();
}
