//! Black-box certification contract for accepted ADR-144 through ADR-148.
//!
//! The suite talks only to the public `decided` binary. It is expected to fail
//! on the v0.28 engine and becomes the acceptance gate for the stacked v0.29
//! graph implementation. Keep failures semantic; do not ignore these tests.

mod federation_graph_support;

use federation_graph_support::{
    constrained_decision, decision, requirement, GraphRepo, OverrideEdge, ParentEdge, ROOT_ID,
    ROOT_SOURCE, SHARED_ID, SHARED_SOURCE,
};
use serde_json::Value;
use std::fs;
use std::process::{Command, Output};

const COMMON_ID: &str = "POL-01K000000001";
const FORBIDDEN_MARKER: &str = "FORBIDDEN_GRAPH_MARKER";

fn run(repo: &GraphRepo, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_decided"))
        .args(args)
        .current_dir(repo.root())
        .env("DECIDED_CACHE_DIR", repo.root().join(".decided/cache"))
        .env("XDG_CACHE_HOME", repo.root().join(".xdg/cache"))
        .env("XDG_CONFIG_HOME", repo.root().join(".xdg/config"))
        .env("XDG_STATE_HOME", repo.root().join(".xdg/state"))
        .output()
        .expect("run decided graph contract")
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn rendered(output: &Output) -> String {
    format!("{}{}", stdout(output), stderr(output))
}

fn assert_success(output: &Output, context: &str) {
    assert!(
        output.status.success(),
        "{context} failed with {:?}\nstdout:\n{}\nstderr:\n{}",
        output.status.code(),
        stdout(output),
        stderr(output)
    );
}

fn assert_stable_failure(output: &Output, code: &str, context: &str) {
    assert_eq!(
        output.status.code(),
        Some(1),
        "{context}\nstdout:\n{}\nstderr:\n{}",
        stdout(output),
        stderr(output)
    );
    assert!(
        rendered(output).contains(code),
        "{context} omitted stable code {code:?}:\n{}",
        rendered(output)
    );
}

fn json(output: &Output, context: &str) -> Value {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "{context} did not emit JSON: {error}\nstdout:\n{}\nstderr:\n{}",
            stdout(output),
            stderr(output)
        )
    })
}

fn json_lines(output: &Output, context: &str) -> Vec<Value> {
    stdout(output)
        .lines()
        .map(|line| {
            serde_json::from_str(line)
                .unwrap_or_else(|error| panic!("{context} emitted invalid JSONL: {error}: {line}"))
        })
        .collect()
}

fn count_id(records: &[Value], id: &str) -> usize {
    records
        .iter()
        .filter(|record| record["id"].as_str() == Some(id))
        .count()
}

fn direct_parent_repo(label: &str, sources: &[(&str, &str, &str)]) -> GraphRepo {
    let repo = GraphRepo::new(label);
    let mut parents = Vec::new();
    for (index, (alias, source, id)) in sources.iter().enumerate() {
        let node = format!("vendor/{alias}");
        let key = format!("P{index:02}");
        repo.create_node(&node, &key, source);
        repo.write_node(
            &node,
            &format!("decisions/{alias}.md"),
            &decision(id, &format!("{alias} inherited policy"), None),
        );
        parents.push(ParentEdge::new(
            alias,
            source,
            &node,
            repo.v2_digest(&node, source),
        ));
    }
    repo.write_v2_manifest("", &parents, &[]);
    repo
}

#[test]
fn manifest_v2_digest_has_a_known_vector_and_binds_exact_bytes() {
    let repo = GraphRepo::new("digest-vector");
    repo.create_node("vendor/vector", "VEC", "acme/vector");
    repo.write_node(
        "vendor/vector",
        "decisions/vector.md",
        &decision("VEC-01K000000001", "Vector Policy", None),
    );

    let expected = repo.v2_digest("vendor/vector", "acme/vector");
    assert_eq!(
        expected, "sha256-v2:d6ca980630e9819dd079d68f639c705ec58244b0e2de7af26fcf6188d7419d45",
        "the independent vector changed; review the exact framing before updating it"
    );
    repo.create_node("vendor/vector/vendor/leaf", "LEF", "acme/vector-leaf");
    repo.write_node(
        "vendor/vector/vendor/leaf",
        "decisions/leaf.md",
        &decision("LEF-01K000000001", "Vector Leaf", None),
    );
    repo.write_v2_manifest(
        "vendor/vector",
        &[ParentEdge::new(
            "leaf",
            "acme/vector-leaf",
            "vendor/leaf",
            repo.v2_digest("vendor/vector/vendor/leaf", "acme/vector-leaf"),
        )],
        &[],
    );
    let manifest_expected = repo.v2_digest("vendor/vector", "acme/vector");
    assert_eq!(
        manifest_expected,
        "sha256-v2:577e8965b30513c36b74008df3b4521f206b2bf2f959b92036e0e3bda7e03c4a",
        "the manifest-present vector changed; review the exact bytes before updating it"
    );

    fs::remove_file(repo.path("vendor/vector/.decided/corpus.md"))
        .expect("temporarily restore the manifest-absent vector");
    let absent_output = run(
        &repo,
        &[
            "corpus",
            "digest",
            "--version",
            "2",
            "--root",
            "vendor/vector",
            "--corpus",
            "decisions",
        ],
    );
    assert_success(&absent_output, "calculate the manifest-absent v2 vector");
    assert_eq!(stdout(&absent_output), format!("{expected}\n"));

    repo.write_v2_manifest(
        "vendor/vector",
        &[ParentEdge::new(
            "leaf",
            "acme/vector-leaf",
            "vendor/leaf",
            repo.v2_digest("vendor/vector/vendor/leaf", "acme/vector-leaf"),
        )],
        &[],
    );
    let present_output = run(
        &repo,
        &[
            "corpus",
            "digest",
            "--version",
            "2",
            "--root",
            "vendor/vector",
            "--corpus",
            "decisions",
        ],
    );
    assert_success(&present_output, "calculate the manifest-present v2 vector");
    assert_eq!(stdout(&present_output), format!("{manifest_expected}\n"));

    let manifest_path = repo.path("vendor/vector/.decided/corpus.md");
    let mut commented = fs::read_to_string(&manifest_path).expect("read vector manifest");
    commented.push_str("\n<!-- exact bytes are authenticated -->\n");
    fs::write(manifest_path, commented).expect("write exact-byte vector change");
    assert_ne!(
        repo.v2_digest("vendor/vector", "acme/vector"),
        manifest_expected
    );
}

#[test]
fn two_and_three_direct_parents_are_source_ordered_not_manifest_ordered() {
    let repo = direct_parent_repo(
        "direct-parents",
        &[
            ("security", "acme/security", "SEC-01K000000001"),
            ("standards", "acme/standards", "STD-01K000000001"),
            ("product", "acme/product", "PRD-01K000000001"),
        ],
    );
    let first = run(&repo, &["export", "decisions", "--documents"]);
    assert_success(&first, "export three direct parents");
    for expected in [
        ROOT_SOURCE,
        "acme/security",
        "acme/standards",
        "acme/product",
    ] {
        assert!(stdout(&first).contains(expected), "missing {expected}");
    }

    let parents = [
        ParentEdge::new(
            "product",
            "acme/product",
            "vendor/product",
            repo.v2_digest("vendor/product", "acme/product"),
        ),
        ParentEdge::new(
            "security",
            "acme/security",
            "vendor/security",
            repo.v2_digest("vendor/security", "acme/security"),
        ),
        ParentEdge::new(
            "standards",
            "acme/standards",
            "vendor/standards",
            repo.v2_digest("vendor/standards", "acme/standards"),
        ),
    ];
    repo.write_v2_manifest("", &parents, &[]);
    let permuted = run(&repo, &["export", "decisions", "--documents"]);
    assert_success(&permuted, "export a permuted direct-parent list");
    assert_eq!(
        first.stdout, permuted.stdout,
        "manifest order changed output"
    );
    assert_eq!(
        first.stderr, permuted.stderr,
        "manifest order changed findings"
    );

    repo.write_v2_manifest("", &parents[..2], &[]);
    let two = run(&repo, &["validate", "decisions", "--json"]);
    assert_success(&two, "validate two direct parents");
}

fn diamond_repo(label: &str) -> (GraphRepo, Vec<ParentEdge>) {
    let repo = GraphRepo::new(label);
    repo.create_node("vendor/a", "BRA", "acme/branch-a");
    repo.create_node("vendor/a/vendor/shared", "SHR", SHARED_SOURCE);
    repo.write_node(
        "vendor/a/vendor/shared",
        "decisions/shared.md",
        &decision(SHARED_ID, "Shared Diamond Policy", None),
    );
    repo.create_node("vendor/b", "BRB", "acme/branch-b");
    repo.copy_node("vendor/a/vendor/shared", "vendor/b/vendor/shared");

    for (branch, source, alias) in [
        ("vendor/a", "acme/branch-a", "shared-a"),
        ("vendor/b", "acme/branch-b", "shared-b"),
    ] {
        repo.write_node(
            branch,
            "decisions/branch.md",
            &decision(
                if branch.ends_with('a') {
                    "BRA-01K000000001"
                } else {
                    "BRB-01K000000001"
                },
                "Branch Policy",
                None,
            ),
        );
        let shared = format!("{branch}/vendor/shared");
        repo.write_v2_manifest(
            branch,
            &[ParentEdge::new(
                alias,
                SHARED_SOURCE,
                "vendor/shared",
                repo.v2_digest(&shared, SHARED_SOURCE),
            )],
            &[],
        );
        assert!(repo.v2_digest(branch, source).starts_with("sha256-v2:"));
    }
    let parents = vec![
        ParentEdge::new(
            "a",
            "acme/branch-a",
            "vendor/a",
            repo.v2_digest("vendor/a", "acme/branch-a"),
        ),
        ParentEdge::new(
            "b",
            "acme/branch-b",
            "vendor/b",
            repo.v2_digest("vendor/b", "acme/branch-b"),
        ),
    ];
    repo.write_v2_manifest("", &parents, &[]);
    (repo, parents)
}

#[test]
fn transitive_diamond_dedupes_equal_pins_and_rejects_divergent_pins() {
    let (repo, mut root_parents) = diamond_repo("diamond");
    let valid = run(&repo, &["export", "decisions", "--documents"]);
    assert_success(&valid, "compose a same-pin transitive diamond");
    let documents = json_lines(&valid, "diamond documents export");
    assert_eq!(
        count_id(&documents, SHARED_ID),
        1,
        "a shared logical node must enter the catalog once"
    );

    repo.write_node(
        "vendor/b/vendor/shared",
        "decisions/shared.md",
        &decision(SHARED_ID, "Divergent Shared Policy", None),
    );
    let divergent_shared = repo.v2_digest("vendor/b/vendor/shared", SHARED_SOURCE);
    repo.write_v2_manifest(
        "vendor/b",
        &[ParentEdge::new(
            "shared-b",
            SHARED_SOURCE,
            "vendor/shared",
            divergent_shared,
        )],
        &[],
    );
    root_parents[1].digest = repo.v2_digest("vendor/b", "acme/branch-b");
    repo.write_v2_manifest("", &root_parents, &[]);
    let invalid = run(&repo, &["validate", "decisions", "--json"]);
    assert_stable_failure(
        &invalid,
        "corpus-federation-divergent-pin",
        "the same source with different canonical node digests must fail",
    );
    let invalid_payload = json(&invalid, "divergent-pin validation");
    let provenance = &invalid_payload["files"][0]["provenance"];
    assert_eq!(provenance["route_count"], 2);
    assert_eq!(
        provenance["source_route"],
        serde_json::json!([ROOT_SOURCE, "acme/branch-a", SHARED_SOURCE])
    );
    assert_eq!(provenance["source"], ROOT_SOURCE);
    assert_eq!(provenance["layer"], "local");
    assert!(
        provenance.get("pin").is_none(),
        "root-owned composition findings must omit pin: {provenance}"
    );
}

#[test]
fn active_source_recurrence_is_reported_as_a_cycle_before_divergent_pin() {
    let repo = GraphRepo::new("cycle");
    repo.create_node("vendor/a", "A01", "acme/a");
    repo.create_node("vendor/a/vendor/b", "B01", "acme/b");
    repo.create_node("vendor/a/vendor/b/vendor/a-copy", "A02", "acme/a");
    repo.write_node(
        "vendor/a/vendor/b/vendor/a-copy",
        "decisions/a-copy.md",
        &decision("A02-01K000000001", "Repeated A Source", None),
    );
    let a_copy = repo.v2_digest("vendor/a/vendor/b/vendor/a-copy", "acme/a");
    repo.write_v2_manifest(
        "vendor/a/vendor/b",
        &[ParentEdge::new(
            "a-again",
            "acme/a",
            "vendor/a-copy",
            a_copy,
        )],
        &[],
    );
    let b = repo.v2_digest("vendor/a/vendor/b", "acme/b");
    repo.write_v2_manifest(
        "vendor/a",
        &[ParentEdge::new("b", "acme/b", "vendor/b", b)],
        &[],
    );
    let a = repo.v2_digest("vendor/a", "acme/a");
    repo.write_v2_manifest("", &[ParentEdge::new("a", "acme/a", "vendor/a", a)], &[]);

    let output = run(&repo, &["validate", "decisions", "--json"]);
    assert_stable_failure(
        &output,
        "corpus-federation-cycle",
        "an active-ancestry source recurrence must be a cycle",
    );
    assert!(rendered(&output).contains("acme/a"));
}

#[test]
fn global_qualification_equal_ids_and_contextual_aliases_share_one_resolver() {
    let repo = GraphRepo::new("qualification");
    let mut parents = Vec::new();
    for (branch, branch_source, leaf_source, branch_id) in [
        ("a", "acme/a", "acme/a-shared", "ARE-01K000000001"),
        ("b", "acme/b", "acme/b-shared", "BRE-01K000000001"),
        ("c", "acme/c", "acme/c-shared", "CRE-01K000000001"),
    ] {
        let node = format!("vendor/{branch}");
        let leaf = format!("{node}/vendor/shared");
        let branch_key = format!("BR{}", branch.to_uppercase());
        repo.create_node(&node, &branch_key, branch_source);
        repo.create_node(&leaf, "LEF", leaf_source);
        repo.write_node(
            &node,
            "decisions/equal.md",
            &decision(COMMON_ID, "Source Qualified Equal ID", None),
        );
        repo.write_node(
            &leaf,
            "decisions/leaf.md",
            &decision(branch_id, "Contextual Leaf", None),
        );
        repo.write_node(
            &node,
            "decisions/reference.md",
            &requirement(
                &format!("BR{}-01K000000002", branch.to_uppercase()),
                "Contextual Alias Reference",
                Some(&format!("shared::{branch_id}")),
            ),
        );
        repo.write_v2_manifest(
            &node,
            &[ParentEdge::new(
                "shared",
                leaf_source,
                "vendor/shared",
                repo.v2_digest(&leaf, leaf_source),
            )],
            &[],
        );
        parents.push(ParentEdge::new(
            branch,
            branch_source,
            &node,
            repo.v2_digest(&node, branch_source),
        ));
    }
    repo.write_v2_manifest("", &parents, &[]);

    let relationships = run(
        &repo,
        &["relationships", "decisions", "--validate", "--json"],
    );
    assert_success(
        &relationships,
        "the same nested alias spelling resolves in each owner context",
    );
    for source in ["acme/a", "acme/b", "acme/c"] {
        let qualified = format!("{source}::{COMMON_ID}");
        let output = run(&repo, &["resolve", &qualified, "decisions", "--json"]);
        assert_success(&output, "globally qualify an equal canonical id");
        assert!(stdout(&output).contains(source));
    }
    let ambiguous = run(&repo, &["resolve", COMMON_ID, "decisions", "--json"]);
    assert_eq!(ambiguous.status.code(), Some(1));
    let ambiguity = rendered(&ambiguous).to_lowercase();
    assert!(ambiguity.contains("ambiguous"));
    for source in ["acme/a", "acme/b", "acme/c"] {
        assert!(ambiguity.contains(source));
    }
}

#[test]
fn override_chains_and_explicit_diamond_convergence_retain_lineage() {
    let repo = GraphRepo::new("override-chain");
    repo.create_node("vendor/mid", "MID", "acme/mid");
    repo.create_node("vendor/mid/vendor/base", "BAS", "acme/base");
    repo.write_node(
        "vendor/mid/vendor/base",
        "decisions/base.md",
        &requirement("BAS-01K000000001", "Base Requirement", None),
    );
    repo.write_node(
        "vendor/mid",
        "decisions/replacement.md",
        &requirement("MID-01K000000001", "Mid Replacement", None),
    );
    repo.write_node(
        "vendor/mid",
        "decisions/rationale.md",
        &decision("MID-01K000000002", "Mid Rationale", None),
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
    repo.write(
        "decisions/replacement.md",
        &requirement("APP-01K000000010", "Root Replacement", None),
    );
    repo.write(
        "decisions/rationale.md",
        &decision("APP-01K000000011", "Root Rationale", None),
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

    let effective = run(
        &repo,
        &["resolve", "BAS-01K000000001", "decisions", "--json"],
    );
    assert_success(&effective, "resolve a two-hop override terminal");
    assert_eq!(
        json(&effective, "override terminal")["id"].as_str(),
        Some("APP-01K000000010")
    );
    let historical = run(
        &repo,
        &[
            "resolve",
            "acme/base::BAS-01K000000001",
            "decisions",
            "--json",
        ],
    );
    assert_success(&historical, "retain qualified override history");
    assert_eq!(
        json(&historical, "override history")["id"].as_str(),
        Some("BAS-01K000000001")
    );
    let export = run(&repo, &["export", "decisions", "--documents"]);
    assert_success(&export, "export complete override lineage");
    for expected in [
        "BAS-01K000000001",
        "MID-01K000000001",
        "APP-01K000000010",
        "MID-01K000000002",
        "APP-01K000000011",
        "overridden",
        "replacement",
        "lineage",
    ] {
        assert!(stdout(&export).contains(expected), "missing {expected}");
    }

    let (diamond, root_parents) = diamond_repo("override-convergence");
    for (branch, branch_source, replacement, rationale) in [
        (
            "vendor/a",
            "acme/branch-a",
            "BRA-01K000000010",
            "BRA-01K000000011",
        ),
        (
            "vendor/b",
            "acme/branch-b",
            "BRB-01K000000010",
            "BRB-01K000000011",
        ),
    ] {
        diamond.write_node(
            branch,
            "decisions/replacement.md",
            &decision(replacement, "Branch Replacement", None),
        );
        diamond.write_node(
            branch,
            "decisions/rationale.md",
            &decision(rationale, "Branch Rationale", None),
        );
        diamond.write_v2_manifest(
            branch,
            &[ParentEdge::new(
                "shared",
                SHARED_SOURCE,
                "vendor/shared",
                diamond.v2_digest(&format!("{branch}/vendor/shared"), SHARED_SOURCE),
            )],
            &[OverrideEdge::new(
                &format!("{SHARED_SOURCE}::{SHARED_ID}"),
                replacement,
                rationale,
            )],
        );
        assert!(diamond
            .v2_digest(branch, branch_source)
            .starts_with("sha256-v2:"));
    }
    diamond.write(
        "decisions/converged.md",
        &decision("APP-01K000000020", "Converged Policy", None),
    );
    diamond.write(
        "decisions/convergence-rationale.md",
        &decision("APP-01K000000021", "Convergence Rationale", None),
    );
    let refreshed = [
        ParentEdge::new(
            "a",
            "acme/branch-a",
            "vendor/a",
            diamond.v2_digest("vendor/a", "acme/branch-a"),
        ),
        ParentEdge::new(
            "b",
            "acme/branch-b",
            "vendor/b",
            diamond.v2_digest("vendor/b", "acme/branch-b"),
        ),
    ];
    assert_eq!(root_parents.len(), refreshed.len());
    diamond.write_v2_manifest(
        "",
        &refreshed,
        &[
            OverrideEdge::new(
                "acme/branch-a::BRA-01K000000010",
                "APP-01K000000020",
                "APP-01K000000021",
            ),
            OverrideEdge::new(
                "acme/branch-b::BRB-01K000000010",
                "APP-01K000000020",
                "APP-01K000000021",
            ),
        ],
    );
    let converged = run(&diamond, &["validate", "decisions", "--json"]);
    assert_success(
        &converged,
        "the joining corpus explicitly reconverges both live branch terminals",
    );
}

#[test]
fn transitive_inherited_decisions_enforce_against_root_code() {
    let repo = GraphRepo::new("deep-enforcement");
    repo.create_node("vendor/mid", "MID", "acme/mid");
    repo.create_node("vendor/mid/vendor/guard", "GRD", "acme/guard");
    repo.write_node(
        "vendor/mid/vendor/guard",
        "decisions/guard.md",
        &constrained_decision("GRD-01K000000001", "Deep Guard", FORBIDDEN_MARKER),
    );
    repo.write_v2_manifest(
        "vendor/mid",
        &[ParentEdge::new(
            "guard",
            "acme/guard",
            "vendor/guard",
            repo.v2_digest("vendor/mid/vendor/guard", "acme/guard"),
        )],
        &[],
    );
    repo.write_v2_manifest(
        "",
        &[ParentEdge::new(
            "mid",
            "acme/mid",
            "vendor/mid",
            repo.v2_digest("vendor/mid", "acme/mid"),
        )],
        &[],
    );

    let scope = run(
        &repo,
        &["decisions-for", "src/guarded.rs", "decisions", "--json"],
    );
    assert_success(&scope, "route a transitive inherited Decision");
    assert!(stdout(&scope).contains("GRD-01K000000001"));
    repo.write(
        "src/guarded.rs",
        &format!("pub const MARKER: &str = \"{FORBIDDEN_MARKER}\";\n"),
    );
    let sentry = run(
        &repo,
        &[
            "sentry",
            "decisions",
            "--repository",
            ".",
            "--full",
            "--json",
        ],
    );
    assert_eq!(
        sentry.status.code(),
        Some(1),
        "deep rule must block root code"
    );
    for expected in ["GRD-01K000000001", "inherited-v2-guard", "src/guarded.rs"] {
        assert!(stdout(&sentry).contains(expected), "missing {expected}");
    }
}

#[test]
fn cache_disabled_cold_warm_and_export_reads_share_one_graph_projection() {
    let repo = direct_parent_repo(
        "cache-export-parity",
        &[
            ("security", "acme/security", "SEC-01K000000001"),
            ("standards", "acme/standards", "STD-01K000000001"),
        ],
    );
    let uncached = run(
        &repo,
        &[
            "find",
            "inherited policy",
            "decisions",
            "--no-cache",
            "--json",
        ],
    );
    let graph_store_root = repo.root().join(".decided/cache/store/v3");
    assert!(
        !graph_store_root.exists(),
        "--no-cache must not create the graph store/v3 layout"
    );
    let cold = run(&repo, &["find", "inherited policy", "decisions", "--json"]);
    let generation_directories = || {
        fs::read_dir(&graph_store_root)
            .expect("read graph store/v3 root")
            .map(|entry| entry.expect("read graph generation entry").path())
            .filter(|path| path.is_dir())
            .collect::<Vec<_>>()
    };
    let cold_generations = generation_directories();
    assert_eq!(
        cold_generations.len(),
        1,
        "the cold CLI process must persist exactly one graph generation"
    );
    for segment in ["entries.seg", "graph.seg"] {
        assert!(
            cold_generations[0].join(segment).is_file(),
            "cold graph store omitted {segment}"
        );
    }
    let warm = run(&repo, &["find", "inherited policy", "decisions", "--json"]);
    let human = run(&repo, &["find", "inherited policy", "decisions"]);
    for (name, output) in [("uncached", &uncached), ("cold", &cold), ("warm", &warm)] {
        assert_success(output, &format!("{name} graph search"));
    }
    assert_success(&human, "human graph search");
    let human_stdout = stdout(&human);
    for expected in [
        "source=acme/security layer=inherited pin=sha256-v2:",
        "source=acme/standards layer=inherited pin=sha256-v2:",
    ] {
        assert!(
            human_stdout.contains(expected),
            "human graph search omitted {expected}: {human_stdout}"
        );
    }
    assert_eq!(uncached.stdout, cold.stdout);
    assert_eq!(cold.stdout, warm.stdout);
    assert_eq!(uncached.stderr, cold.stderr);
    assert_eq!(cold.stderr, warm.stderr);
    assert_eq!(
        generation_directories(),
        cold_generations,
        "the second CLI process must reuse the exact store/v3 generation"
    );
    let find_payload = json(&warm, "warm graph search");
    let matches = find_payload["matches"]
        .as_array()
        .expect("graph search matches");
    assert_eq!(matches.len(), 2);
    for matched in matches {
        let provenance = &matched["provenance"];
        assert_eq!(provenance["layer"].as_str(), Some("inherited"));
        assert!(
            provenance["pin"]
                .as_str()
                .is_some_and(|pin| pin.starts_with("sha256-v2:")),
            "inherited CLI find result omitted its v2 pin: {matched}"
        );
        assert!(
            provenance["source"].as_str().is_some(),
            "inherited CLI find result omitted its source: {matched}"
        );
        assert!(
            matched.get("recency").is_none(),
            "inherited CLI find must not derive recency from the root checkout: {matched}"
        );
    }

    let documents = run(&repo, &["export", "decisions", "--documents"]);
    let graph = run(&repo, &["export", "decisions", "--graph"]);
    let viewer = run(&repo, &["export", "decisions"]);
    for (name, output) in [
        ("documents", &documents),
        ("graph", &graph),
        ("viewer", &viewer),
    ] {
        assert_success(output, &format!("{name} graph export"));
        for source in [ROOT_SOURCE, "acme/security", "acme/standards"] {
            assert!(stdout(output).contains(source), "{name} omitted {source}");
        }
    }
    let local = run(
        &repo,
        &["export", "decisions", "--documents", "--local-only"],
    );
    assert_success(&local, "root-local diagnostic export");
    assert!(stdout(&local).contains(ROOT_ID));
    assert!(!stdout(&local).contains("acme/security"));
    assert!(!stdout(&local).contains("acme/standards"));
}

#[test]
fn no_manifest_and_manifest_v1_keep_the_released_observable_contract() {
    let repo = GraphRepo::new("legacy-parity");
    let before = run(&repo, &["resolve", ROOT_ID, "decisions", "--json"]);
    assert_success(&before, "resolve without a manifest");
    repo.create_node("vendor/unconfigured", "IGN", "acme/ignored");
    repo.write_node(
        "vendor/unconfigured",
        "decisions/ignored.md",
        &decision("IGN-01K000000001", "Ignored Physical Corpus", None),
    );
    let after = run(&repo, &["resolve", ROOT_ID, "decisions", "--json"]);
    assert_success(&after, "resolve with an unconfigured physical corpus");
    assert_eq!(before.stdout, after.stdout);
    assert_eq!(before.stderr, after.stderr);

    repo.create_node("vendor/v1", "VON", "acme/v1");
    repo.write_node(
        "vendor/v1",
        "decisions/v1.md",
        &decision("VON-01K000000001", "Version One Policy", None),
    );
    let digest = run(
        &repo,
        &[
            "corpus",
            "digest",
            "--root",
            "vendor/v1",
            "--corpus",
            "decisions",
        ],
    );
    assert_success(&digest, "calculate an unchanged v1 digest");
    let digest = stdout(&digest).trim().to_string();
    assert!(digest.starts_with("sha256:"));
    repo.write(
        ".decided/corpus.md",
        &format!(
            "# Corpus\n\n## inherits\n\n```yaml\nversion: 1\nalias: legacy\nsource: acme/v1\nroot: vendor/v1\ncorpus: decisions\ndigest: {digest}\n```\n"
        ),
    );
    let inherited = run(
        &repo,
        &["resolve", "legacy::VON-01K000000001", "decisions", "--json"],
    );
    assert_success(&inherited, "retain exact v1 qualification");
    assert!(stdout(&inherited).contains("\"source\": \"acme/v1\""));
    assert!(stdout(&inherited).contains(&digest));

    let v1_multiple = fs::read_to_string(repo.root().join(".decided/corpus.md"))
        .expect("read v1 manifest")
        .replace("version: 1", "version: 1\nparents: []");
    repo.write(".decided/corpus.md", &v1_multiple);
    let failure = run(&repo, &["validate", "decisions", "--json"]);
    assert_eq!(failure.status.code(), Some(1));
    assert!(rendered(&failure).contains("parent-corpus"));
}
