//! MCP parity contract for the accepted corpus-federation graph design.
//!
//! Fixture pins are calculated independently of the engine. The same six
//! public tools must consume one request-current graph with or without cache.

#[path = "../../decided/tests/federation_graph_support/mod.rs"]
mod federation_graph_support;

use federation_graph_support::{
    constrained_decision, decision, requirement, GraphRepo, OverrideEdge, ParentEdge,
};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

fn request(id: usize, name: &str, arguments: Value) -> String {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "tools/call",
        "params": {"name": name, "arguments": arguments}
    })
    .to_string()
}

fn run(root: &Path, extra: &[&str], requests: &[String]) -> Vec<Value> {
    let mut child = Command::new(env!("CARGO_BIN_EXE_decided-mcp"))
        .arg("--root")
        .arg(root)
        .args(extra)
        .env("DECIDED_CACHE_DIR", root.join("../.decided/cache"))
        .env("XDG_CACHE_HOME", root.join("../.xdg/cache"))
        .env("XDG_CONFIG_HOME", root.join("../.xdg/config"))
        .env("XDG_STATE_HOME", root.join("../.xdg/state"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn decided-mcp graph contract");
    {
        let stdin = child.stdin.as_mut().expect("MCP stdin");
        for request in requests {
            writeln!(stdin, "{request}").expect("write MCP request");
        }
    }
    drop(child.stdin.take());
    let output = child.wait_with_output().expect("wait for decided-mcp");
    assert!(
        output.status.success(),
        "MCP graph server failed with {:?}: {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .expect("UTF-8 MCP frames")
        .lines()
        .map(|line| serde_json::from_str(line).expect("JSON-RPC response"))
        .collect()
}

fn tool_text(frame: &Value) -> &str {
    frame
        .pointer("/result/content/0/text")
        .and_then(Value::as_str)
        .expect("MCP tool text")
}

fn graph_repo() -> GraphRepo {
    let repo = GraphRepo::new("mcp-parity");
    repo.create_node("vendor/standards", "STD", "acme/standards");
    repo.write_node(
        "vendor/standards",
        "decisions/standard.md",
        &constrained_decision(
            "STD-01K000000001",
            "Quantum Ledger Compaction Standard",
            "forbidden_graph_marker",
        ),
    );
    repo.create_node("vendor/product", "PRD", "acme/product");
    repo.create_node("vendor/product/vendor/shared", "SHR", "acme/shared");
    repo.write_node(
        "vendor/product/vendor/shared",
        "decisions/shared.md",
        &decision("SHR-01K000000001", "Quantum Ledger Shared Guardrail", None),
    );
    repo.write_node(
        "vendor/product",
        "decisions/product.md",
        &decision(
            "PRD-01K000000001",
            "Quantum Ledger Product Policy",
            Some("shared::SHR-01K000000001"),
        ),
    );
    repo.write_v2_manifest(
        "vendor/product",
        &[ParentEdge::new(
            "shared",
            "acme/shared",
            "vendor/shared",
            repo.v2_digest("vendor/product/vendor/shared", "acme/shared"),
        )],
        &[],
    );
    repo.write_v2_manifest(
        "",
        &[
            ParentEdge::new(
                "standards",
                "acme/standards",
                "vendor/standards",
                repo.v2_digest("vendor/standards", "acme/standards"),
            ),
            ParentEdge::new(
                "product",
                "acme/product",
                "vendor/product",
                repo.v2_digest("vendor/product", "acme/product"),
            ),
        ],
        &[],
    );
    repo
}

#[test]
fn all_six_tools_have_cold_warm_and_cache_disabled_graph_parity() {
    let repo = graph_repo();
    let corpus = repo.root().join("decisions");
    let requests = vec![
        request(
            1,
            "get_artifact",
            json!({"id": "acme/standards::STD-01K000000001"}),
        ),
        request(
            2,
            "search_artifacts",
            json!({"query": "quantum ledger"}),
        ),
        request(
            3,
            "retrieve_grounding",
            json!({"task": "quantum ledger compaction", "top_k": 5}),
        ),
        request(
            4,
            "find_decisions",
            json!({"topic": "quantum ledger compaction"}),
        ),
        request(
            5,
            "get_related",
            json!({"id": "acme/product::PRD-01K000000001", "depth": 2}),
        ),
        request(6, "get_summary", json!({})),
        request(
            7,
            "find_decisions",
            json!({"path": "src/guarded.rs"}),
        ),
    ];

    let uncached = run(&corpus, &["--no-cache"], &requests);
    let cold = run(&corpus, &[], &requests);
    let warm = run(&corpus, &[], &requests);
    assert_eq!(uncached.len(), 7);
    assert_eq!(cold.len(), 7);
    assert_eq!(warm.len(), 7);
    assert_eq!(
        uncached.iter().map(tool_text).collect::<Vec<_>>(),
        cold.iter().map(tool_text).collect::<Vec<_>>(),
        "cache-disabled and cold tools observed different graphs"
    );
    assert_eq!(
        cold.iter().map(tool_text).collect::<Vec<_>>(),
        warm.iter().map(tool_text).collect::<Vec<_>>(),
        "cold and store-hit tools observed different graphs"
    );

    let joined = cold.iter().map(tool_text).collect::<Vec<_>>().join("\n");
    for expected in [
        "acme/standards",
        "acme/product",
        "acme/shared",
        "sha256-v2:",
    ] {
        assert!(joined.contains(expected), "MCP output omitted {expected}");
    }
    assert!(
        cold.iter()
            .all(|frame| frame["result"]["isError"] == json!(false)),
        "a graph-capable MCP tool rejected the shared closure: {cold:?}"
    );
}

fn payload(frame: &Value) -> Value {
    serde_json::from_str(tool_text(frame)).expect("tool payload JSON")
}

struct LiveServer {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}

impl LiveServer {
    fn start(root: &Path) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_decided-mcp"))
            .args(["--root", root.to_str().unwrap(), "--no-cache"])
            .env("XDG_CACHE_HOME", root.join("../.xdg/cache"))
            .env("XDG_CONFIG_HOME", root.join("../.xdg/config"))
            .env("XDG_STATE_HOME", root.join("../.xdg/state"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn live MCP graph server");
        let stdin = child.stdin.take().expect("live MCP stdin");
        let stdout = BufReader::new(child.stdout.take().expect("live MCP stdout"));
        Self { child, stdin, stdout }
    }

    fn send(&mut self, value: &Value) -> Value {
        writeln!(self.stdin, "{value}").expect("write live MCP request");
        self.stdin.flush().expect("flush live MCP request");
        let mut line = String::new();
        self.stdout.read_line(&mut line).expect("read live MCP frame");
        assert!(!line.is_empty(), "live MCP process closed unexpectedly");
        serde_json::from_str(&line).expect("live MCP JSON-RPC frame")
    }
}

impl Drop for LiveServer {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
fn override_chain_is_atomic_and_historical_reads_are_explicit() {
    let repo = GraphRepo::new("mcp-override-chain");
    repo.create_node("vendor/mid", "MID", "acme/mid");
    repo.create_node("vendor/mid/vendor/base", "BAS", "acme/base");
    repo.write_node(
        "vendor/mid/vendor/base",
        "decisions/base.md",
        &requirement("BAS-01K000000001", "Graph Chain Base", None),
    );
    repo.write_node(
        "vendor/mid",
        "decisions/replacement.md",
        &requirement("MID-01K000000001", "Graph Chain Middle", None),
    );
    repo.write_node(
        "vendor/mid",
        "decisions/rationale.md",
        &decision("MID-01K000000002", "Graph Chain Rationale", None),
    );
    repo.write_v2_manifest(
        "vendor/mid",
        &[ParentEdge::new(
            "base",
            "acme/base",
            "vendor/base",
            repo.v2_digest("vendor/mid/vendor/base", "acme/base"),
        )],
        &[OverrideEdge::new(
            "acme/base::BAS-01K000000001",
            "MID-01K000000001",
            "MID-01K000000002",
        )],
    );
    let oversized = format!(
        "{}\n{}\n",
        requirement("APP-01K000000010", "Graph Chain Terminal", None),
        "terminal body ".repeat(600)
    );
    repo.write("decisions/replacement.md", &oversized);
    repo.write(
        "decisions/rationale.md",
        &decision("APP-01K000000011", "Root Chain Rationale", None),
    );
    repo.write_v2_manifest(
        "",
        &[ParentEdge::new(
            "mid",
            "acme/mid",
            "vendor/mid",
            repo.v2_digest("vendor/mid", "acme/mid"),
        )],
        &[OverrideEdge::new(
            "acme/mid::MID-01K000000001",
            "APP-01K000000010",
            "APP-01K000000011",
        )],
    );
    let corpus = repo.root().join("decisions");
    let frames = run(
        &corpus,
        &["--no-cache"],
        &[
            request(1, "get_artifact", json!({"id": "BAS-01K000000001"})),
            request(
                2,
                "get_artifact",
                json!({"id": "acme/base::BAS-01K000000001"}),
            ),
            request(
                3,
                "get_artifact",
                json!({"id": "APP-01K000000010", "budget": 1024}),
            ),
        ],
    );
    let effective = payload(&frames[0]);
    assert_eq!(effective["id"], json!("APP-01K000000010"));
    assert_eq!(effective["provenance"]["overrides"].as_array().unwrap().len(), 2);
    let historical = payload(&frames[1]);
    assert_eq!(historical["id"], json!("BAS-01K000000001"));
    assert_eq!(historical["provenance"]["source"], json!("acme/base"));
    assert_eq!(historical["provenance"]["overrides"].as_array().unwrap().len(), 2);

    let bounded = payload(&frames[2]);
    if bounded.get("error").is_some() {
        assert!(bounded.to_string().contains("response_budget_exceeded"));
    } else {
        assert_eq!(bounded["provenance"]["overrides"].as_array().unwrap().len(), 2);
    }
}

#[test]
fn diamond_equal_paths_and_equal_ids_remain_source_aware() {
    const COMMON: &str = "COM-01K000000001";
    let repo = GraphRepo::new("mcp-diamond-identity");
    let mut parents = Vec::new();
    for (branch, source, key) in [("a", "acme/a", "A01"), ("b", "acme/b", "B01")] {
        let node = format!("vendor/{branch}");
        let shared = format!("{node}/vendor/shared");
        repo.create_node(&node, key, source);
        repo.create_node(&shared, "SHR", "acme/shared");
        repo.write_node(
            &shared,
            "decisions/shared.md",
            &decision("SHR-01K000000001", "Diamond Shared Policy", None),
        );
        repo.write_node(
            &node,
            "decisions/equal.md",
            &decision(COMMON, &format!("Equal Identity {branch}"), None),
        );
        repo.write_v2_manifest(
            &node,
            &[ParentEdge::new(
                "shared",
                "acme/shared",
                "vendor/shared",
                repo.v2_digest(&shared, "acme/shared"),
            )],
            &[],
        );
        parents.push(ParentEdge::new(
            branch,
            source,
            &node,
            repo.v2_digest(&node, source),
        ));
    }
    repo.write_v2_manifest("", &parents, &[]);
    let corpus = repo.root().join("decisions");
    let frames = run(
        &corpus,
        &["--no-cache"],
        &[
            request(1, "search_artifacts", json!({"query": "equal identity"})),
            request(2, "get_artifact", json!({"id": COMMON})),
            request(3, "get_artifact", json!({"id": format!("acme/a::{COMMON}")})),
            request(4, "get_artifact", json!({"id": format!("b::{COMMON}")})),
            request(5, "get_artifact", json!({"id": "acme/shared::SHR-01K000000001"})),
        ],
    );
    let matches = payload(&frames[0]);
    let sources = matches["matches"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["provenance"]["source"].as_str().unwrap())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(sources, std::collections::BTreeSet::from(["acme/a", "acme/b"]));
    assert!(matches["matches"]
        .as_array()
        .unwrap()
        .iter()
        .all(|item| item.get("recency").is_none()
            && item["provenance"]["layer"] == json!("inherited")
            && item["provenance"]["pin"]
                .as_str()
                .is_some_and(|pin| pin.starts_with("sha256-v2:"))));
    assert!(payload(&frames[1]).to_string().contains("ambiguous"));
    assert_eq!(payload(&frames[2])["provenance"]["source"], json!("acme/a"));
    assert_eq!(payload(&frames[3])["provenance"]["source"], json!("acme/b"));
    assert_eq!(payload(&frames[4])["provenance"]["source"], json!("acme/shared"));
}

#[test]
fn tamper_and_topology_changes_fail_closed_and_are_audited_once() {
    let repo = GraphRepo::new("mcp-fail-closed");
    let audit_path = repo.root().join("audit.jsonl");
    repo.write(
        ".decided/config.yaml",
        &format!(
            "repository_key: APP\ncorpus:\n  source: acme/root\naudit:\n  enabled: true\n  path: {}\n",
            audit_path.display()
        ),
    );
    repo.create_node("vendor/parent", "PAR", "acme/parent");
    let parent_text = decision("PAR-01K000000001", "Fail Closed Parent", None);
    repo.write_node("vendor/parent", "decisions/parent.md", &parent_text);
    repo.write_v2_manifest(
        "",
        &[ParentEdge::new(
            "parent",
            "acme/parent",
            "vendor/parent",
            repo.v2_digest("vendor/parent", "acme/parent"),
        )],
        &[],
    );
    let mut server = LiveServer::start(&repo.root().join("decisions"));
    let call = |id| {
        json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "tools/call",
            "params": {"name": "search_artifacts", "arguments": {"query": "fail closed parent"}}
        })
    };

    let first = server.send(&call(1));
    assert_eq!(first["result"]["isError"], json!(false));
    repo.write_node(
        "vendor/parent",
        "decisions/parent.md",
        "changed after verification\n",
    );
    let stale = server.send(&call(2));
    assert_eq!(stale["result"]["isError"], json!(true));
    assert!(tool_text(&stale).contains("parent-corpus-digest-mismatch"));

    repo.write_node("vendor/parent", "decisions/parent.md", &parent_text);
    let restored = server.send(&call(3));
    assert_eq!(restored["result"]["isError"], json!(false));
    std::fs::remove_file(repo.root().join(".decided/corpus.md")).expect("remove manifest");
    let topology = server.send(&call(4));
    assert_eq!(topology["result"]["isError"], json!(true));
    assert!(tool_text(&topology).contains("manifest disappeared"));

    let list = server.send(&json!({"jsonrpc":"2.0","id":5,"method":"tools/list"}));
    assert_eq!(list["result"]["tools"].as_array().unwrap().len(), 6);
    let events = std::fs::read_to_string(&audit_path)
        .expect("read graph audit")
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).expect("audit JSON"))
        .collect::<Vec<_>>();
    assert_eq!(events.len(), 4, "one event per recognized tool call");
    assert_eq!(events[0]["outcome"], json!("ok"));
    assert_eq!(events[1]["outcome"], json!("error"));
    assert_eq!(events[1]["returned"], json!([]));
    assert_eq!(events[2]["outcome"], json!("ok"));
    assert_eq!(events[3]["outcome"], json!("error"));
    assert_eq!(events[3]["returned"], json!([]));
}

#[test]
fn malformed_live_manifest_presence_is_sticky_before_version_selection() {
    let repo = GraphRepo::new("mcp-malformed-sticky");
    let mut server = LiveServer::start(&repo.root().join("decisions"));
    let call = |id| {
        json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "tools/call",
            "params": {"name": "get_summary", "arguments": {}}
        })
    };
    assert_eq!(server.send(&call(1))["result"]["isError"], json!(false));
    repo.write(
        ".decided/corpus.md",
        "# Corpus\n\n## inherits\n\n```yaml\nversion: 2\nparents: [\n```\n",
    );
    let malformed = server.send(&call(2));
    assert_eq!(malformed["result"]["isError"], json!(true));
    std::fs::remove_file(repo.root().join(".decided/corpus.md")).expect("remove malformed manifest");
    let removed = server.send(&call(3));
    assert_eq!(removed["result"]["isError"], json!(true));
    assert!(tool_text(&removed).contains("manifest disappeared"));
}
