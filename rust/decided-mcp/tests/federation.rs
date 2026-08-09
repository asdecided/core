use serde_json::{json, Value};
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};

static COUNTER: AtomicUsize = AtomicUsize::new(0);

fn scratch(tag: &str) -> PathBuf {
    let sequence = COUNTER.fetch_add(1, Ordering::SeqCst);
    let root = std::env::temp_dir().join(format!(
        "decided-mcp-federation-{tag}-{}-{sequence}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).expect("create federation scratch directory");
    root
}

fn copy_tree(source: &Path, target: &Path) {
    fs::create_dir_all(target).expect("create copied directory");
    for entry in fs::read_dir(source).expect("read fixture directory") {
        let entry = entry.expect("read fixture entry");
        let destination = target.join(entry.file_name());
        if entry.file_type().expect("fixture file type").is_dir() {
            copy_tree(&entry.path(), &destination);
        } else {
            fs::copy(entry.path(), destination).expect("copy fixture file");
        }
    }
}

fn eval_fixture(tag: &str) -> PathBuf {
    let target = scratch(tag);
    let source = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../fixtures/eval/federation/child");
    copy_tree(&source, &target);
    target
}

fn request(id: usize, name: &str, arguments: Value) -> String {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "tools/call",
        "params": {"name": name, "arguments": arguments}
    })
    .to_string()
}

fn run(root: &Path, extra_args: &[&str], requests: &[String]) -> Vec<Value> {
    let mut child = Command::new(env!("CARGO_BIN_EXE_decided-mcp"))
        .arg("--root")
        .arg(root)
        .args(extra_args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn decided-mcp");
    {
        let stdin = child.stdin.as_mut().expect("server stdin");
        for request in requests {
            writeln!(stdin, "{request}").expect("write MCP request");
        }
    }
    drop(child.stdin.take());
    let output = child.wait_with_output().expect("wait for decided-mcp");
    assert!(
        output.status.success(),
        "server failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .expect("UTF-8 MCP output")
        .lines()
        .map(|line| serde_json::from_str(line).expect("JSON-RPC response"))
        .collect()
}

fn tool_text(frame: &Value) -> &str {
    frame
        .pointer("/result/content/0/text")
        .and_then(Value::as_str)
        .expect("tool text")
}

fn tool_value(frame: &Value) -> Value {
    serde_json::from_str(tool_text(frame)).expect("tool payload JSON")
}

#[test]
fn all_six_tools_share_one_verified_composition_with_or_without_cache() {
    let repository = eval_fixture("six-tools");
    let corpus = repository.join("decisions");
    let requests = vec![
        request(
            1,
            "get_artifact",
            json!({"id": "standards::FEDEVAL-000000000001"}),
        ),
        request(
            2,
            "search_artifacts",
            json!({"query": "quantum ledger compaction"}),
        ),
        request(
            3,
            "retrieve_grounding",
            json!({"task": "quantum ledger compaction", "top_k": 3}),
        ),
        request(
            4,
            "find_decisions",
            json!({"topic": "quantum ledger compaction"}),
        ),
        request(
            5,
            "get_related",
            json!({"id": "standards::FEDEVAL-000000000002", "depth": 2}),
        ),
        request(6, "get_summary", json!({})),
    ];

    let uncached = run(&corpus, &["--no-cache"], &requests);
    let cached = run(&corpus, &[], &requests);
    assert_eq!(
        uncached.iter().map(tool_text).collect::<Vec<_>>(),
        cached.iter().map(tool_text).collect::<Vec<_>>()
    );

    let artifact = tool_value(&cached[0]);
    assert_eq!(artifact["provenance"]["source"], json!("eval/standards"));
    assert_eq!(artifact["provenance"]["layer"], json!("inherited"));
    assert!(artifact["provenance"]["pin"]
        .as_str()
        .is_some_and(|pin| pin.starts_with("sha256:")));

    let search = tool_value(&cached[1]);
    assert_eq!(search["matches"][0]["id"], json!("FEDEVAL-000000000001"));
    assert_eq!(
        search["matches"][0]["provenance"]["source"],
        json!("eval/standards")
    );
    assert!(search["matches"].as_array().unwrap().iter().any(|record| {
        record["provenance"]["source"] == json!("eval/child")
            && record.get("recency").is_some()
    }));
    assert!(search["matches"].as_array().unwrap().iter().all(|record| {
        record["provenance"]["source"] != json!("eval/standards")
            || record.get("recency").is_none()
    }));
    let grounding = tool_value(&cached[2]);
    assert_eq!(
        grounding["items"][0]["provenance"]["layer"],
        json!("inherited")
    );
    assert_eq!(tool_value(&cached[5])["artifacts"]["total"], json!(41));
    fs::remove_dir_all(repository).expect("remove six-tool fixture");
}

fn decision(id: &str, title: &str) -> String {
    format!(
        "---\nschema_version: 1\nid: {id}\ntype: decision\n---\n# {title}\n\n## Status\n\nAccepted\n\n## Context\n\nA reviewed context.\n\n## Decision\n\nKeep the reviewed rule.\n\n## Consequences\n\nThe rule is deterministic.\n"
    )
}

fn override_fixture() -> PathBuf {
    let child = scratch("override");
    let parent = child.join("vendor/standards");
    fs::create_dir_all(parent.join(".decided")).expect("parent config directory");
    fs::create_dir_all(parent.join("decisions")).expect("parent corpus directory");
    fs::create_dir_all(child.join(".decided")).expect("child config directory");
    fs::create_dir_all(child.join("decisions")).expect("child corpus directory");
    fs::write(
        parent.join(".decided/config.yaml"),
        "repository_key: STD\ncorpus:\n  source: acme/standards\n",
    )
    .expect("parent config");
    fs::write(
        parent.join("decisions/parent.md"),
        decision("STD-01JY4M8X2QZ7", "Parent Policy"),
    )
    .expect("parent decision");
    fs::write(
        child.join(".decided/config.yaml"),
        "repository_key: APP\ncorpus:\n  source: acme/app\n",
    )
    .expect("child config");
    fs::write(
        child.join("decisions/replacement.md"),
        decision("APP-01JY4M8X2QZ8", "Local Replacement"),
    )
    .expect("replacement decision");
    fs::write(
        child.join("decisions/rationale.md"),
        decision("APP-01JY4M8X2QZ9", "Override Rationale"),
    )
    .expect("rationale decision");
    let digest = rac_engine::federation::calculate_parent_digest(&parent, "decisions")
        .expect("calculate parent digest")
        .digest;
    fs::write(
        child.join(".decided/corpus.md"),
        format!(
            "# Corpus\n\n## inherits\n\n```yaml\nversion: 1\nalias: standards\nsource: acme/standards\nroot: vendor/standards\ncorpus: decisions\ndigest: {digest}\n```\n\n## overrides\n\n```yaml\nversion: 1\nitems:\n  - parent: standards::STD-01JY4M8X2QZ7\n    with: APP-01JY4M8X2QZ8\n    rationale: APP-01JY4M8X2QZ9\n```\n"
        ),
    )
    .expect("child manifest");
    child
}

#[test]
fn qualified_history_and_canonical_redirect_keep_complete_override_provenance() {
    let repository = override_fixture();
    let corpus = repository.join("decisions");
    let frames = run(
        &corpus,
        &["--no-cache"],
        &[
            request(
                1,
                "get_artifact",
                json!({"id": "standards::STD-01JY4M8X2QZ7"}),
            ),
            request(2, "get_artifact", json!({"id": "STD-01JY4M8X2QZ7"})),
        ],
    );
    let parent = tool_value(&frames[0]);
    let replacement = tool_value(&frames[1]);
    assert_eq!(parent["id"], json!("STD-01JY4M8X2QZ7"));
    assert_eq!(parent["provenance"]["overrides"][0]["state"], json!("overridden"));
    assert_eq!(replacement["id"], json!("APP-01JY4M8X2QZ8"));
    let mapping = &replacement["provenance"]["overrides"][0];
    assert_eq!(mapping["state"], json!("replacement"));
    assert_eq!(mapping["parent"]["source"], json!("acme/standards"));
    assert_eq!(mapping["replacement"]["source"], json!("acme/app"));
    assert_eq!(mapping["rationale"]["id"], json!("APP-01JY4M8X2QZ9"));
    fs::remove_dir_all(repository).expect("remove override fixture");
}

#[test]
fn tight_budgets_keep_parent_and_replacement_override_provenance_atomic() {
    let repository = override_fixture();
    let corpus = repository.join("decisions");
    let mut requests = Vec::new();
    let mut expectations = Vec::new();
    for budget in [384, 512, 768] {
        requests.push(request(
            requests.len() + 1,
            "get_artifact",
            json!({"id": "standards::STD-01JY4M8X2QZ7", "budget": budget}),
        ));
        expectations.push((budget, "overridden"));
        requests.push(request(
            requests.len() + 1,
            "get_artifact",
            json!({"id": "STD-01JY4M8X2QZ7", "budget": budget}),
        ));
        expectations.push((budget, "replacement"));
    }

    let frames = run(&corpus, &["--no-cache"], &requests);
    for (frame, (budget, state)) in frames.iter().zip(expectations) {
        let text = tool_text(frame);
        assert!(
            text.chars().count() <= budget,
            "{} characters exceeded budget {budget}",
            text.chars().count()
        );
        let value = tool_value(frame);
        if value["error"] == json!(rac_engine::budget::BUDGET_ERROR) {
            continue;
        }
        let mapping = &value["provenance"]["overrides"][0];
        assert_eq!(mapping["state"], json!(state));
        assert_eq!(mapping["parent"]["source"], json!("acme/standards"));
        assert_eq!(mapping["parent"]["id"], json!("STD-01JY4M8X2QZ7"));
        assert_eq!(mapping["replacement"]["source"], json!("acme/app"));
        assert_eq!(mapping["replacement"]["id"], json!("APP-01JY4M8X2QZ8"));
        assert_eq!(mapping["rationale"]["source"], json!("acme/app"));
        assert_eq!(mapping["rationale"]["id"], json!("APP-01JY4M8X2QZ9"));
    }
    fs::remove_dir_all(repository).expect("remove tight-budget override fixture");
}

#[test]
fn stale_parent_blocks_the_next_request_instead_of_serving_the_old_generation() {
    let repository = eval_fixture("stale-parent");
    let corpus = repository.join("decisions");
    let mut child = Command::new(env!("CARGO_BIN_EXE_decided-mcp"))
        .arg("--root")
        .arg(&corpus)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn long-lived decided-mcp");
    let mut stdin = child.stdin.take().expect("server stdin");
    let mut stdout = BufReader::new(child.stdout.take().expect("server stdout"));
    let query = request(
        1,
        "search_artifacts",
        json!({"query": "quantum ledger compaction"}),
    );
    writeln!(stdin, "{query}").expect("write first request");
    stdin.flush().expect("flush first request");
    let mut first_line = String::new();
    stdout.read_line(&mut first_line).expect("read first response");
    let first: Value = serde_json::from_str(first_line.trim()).expect("first response JSON");
    assert_eq!(first["result"]["isError"], json!(false));

    let parent_file = repository
        .join("vendor/standards/decisions/quantum-ledger-compaction-anchor.md");
    fs::write(&parent_file, "changed after verification\n").expect("mutate parent bytes");
    let second = request(
        2,
        "search_artifacts",
        json!({"query": "quantum ledger compaction"}),
    );
    writeln!(stdin, "{second}").expect("write second request");
    stdin.flush().expect("flush second request");
    let mut second_line = String::new();
    stdout
        .read_line(&mut second_line)
        .expect("read second response");
    let second: Value = serde_json::from_str(second_line.trim()).expect("second response JSON");
    assert_eq!(second["result"]["isError"], json!(true));
    assert!(tool_text(&second).contains("parent-corpus-digest-mismatch"));
    assert!(!tool_text(&second).contains("FEDEVAL-000000000001"));

    drop(stdin);
    let output = child.wait_with_output().expect("wait for long-lived server");
    assert!(
        output.status.success(),
        "server failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    fs::remove_dir_all(repository).expect("remove stale-parent fixture");
}
