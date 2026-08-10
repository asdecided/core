use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use rac_engine::export::{CorpusExport, ExportArtifact, ExportRelationship};
use rac_engine::okf::{render_okf_bundle, ArtifactRecency};

static NEXT_DIR: AtomicUsize = AtomicUsize::new(0);

fn fixture_dir() -> PathBuf {
    let suffix = NEXT_DIR.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("asdecided-okf-v02-{}-{suffix}", std::process::id()))
}

fn artifact(path: String, id: &str, status: &str, title: &str, tags: &[&str]) -> ExportArtifact {
    ExportArtifact {
        id: id.to_string(),
        aliases: vec![id.to_string()],
        artifact_type: "decision".to_string(),
        status: status.to_string(),
        title: title.to_string(),
        path,
        body_html: String::new(),
        tags: tags.iter().map(|tag| (*tag).to_string()).collect(),
        provenance: None,
    }
}

#[test]
fn exports_truthful_okf_v02_carrier() {
    let root = fixture_dir();
    fs::create_dir_all(&root).unwrap();
    let first = root.join("first.md");
    let second = root.join("second.md");
    fs::write(&first, "---\ntype: decision\n---\n# First\n").unwrap();
    fs::write(&second, "---\ntype: decision\n---\n# Second\n").unwrap();

    let export = CorpusExport {
        corpus_name: "Test corpus".to_string(),
        corpus_source: "test/corpus".to_string(),
        rac_version: "0.25.0".to_string(),
        artifacts: vec![
            artifact(
                first.to_string_lossy().into_owned(),
                "RAC-01JY4M8X2QZ7",
                "Proposed",
                "First: \"quoted\"",
                &["safe", "colon: value"],
            ),
            artifact(
                second.to_string_lossy().into_owned(),
                "RAC-01JY4M8X2QZ8",
                "Superseded",
                "Second",
                &[],
            ),
        ],
        relationships: vec![ExportRelationship {
            from: "RAC-01JY4M8X2QZ7".to_string(),
            to: "RAC-01JY4M8X2QZ8".to_string(),
            edge_type: "related decisions".to_string(),
            from_identity: None,
            to_identity: None,
            provenance: None,
        }],
    };
    let recency = vec![
        ArtifactRecency {
            path: first.to_string_lossy().into_owned(),
            first_committed: Some("2026-07-28T10:00:00+01:00".to_string()),
            last_committed: Some("2026-07-29T11:00:00+01:00".to_string()),
        },
        ArtifactRecency {
            path: second.to_string_lossy().into_owned(),
            first_committed: None,
            last_committed: None,
        },
    ];

    let bundle = render_okf_bundle(&export, &recency, root.to_str().unwrap()).unwrap();
    let index = &bundle["index.md"];
    assert!(index.starts_with("---\nokf_version: \"0.2\"\n---\n"));

    let first_out = &bundle["first.md"];
    assert!(first_out.contains("title: \"First: \\\"quoted\\\"\""));
    assert!(first_out.contains("status: draft"));
    assert!(first_out.contains("generated:\n  by: asdecided/"));
    assert!(first_out.contains("  at: 2026-07-29T11:00:00+01:00"));
    assert!(first_out.contains("tags: [\"safe\",\"colon: value\"]"));
    assert!(first_out.contains("# Related concepts\n\n- [Second](second.md)"));

    for invented in [
        "sources:",
        "verified:",
        "stale_after:",
        "attester:",
        "created:",
        "updated:",
        "# Citations",
    ] {
        assert!(!first_out.contains(invented), "must not emit {invented}");
    }

    let second_out = &bundle["second.md"];
    assert!(second_out.contains("status: deprecated"));
    assert!(!second_out.contains("generated:"));

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn retains_v01_shape_only_as_a_retirement_fixture() {
    let legacy = include_str!("fixtures/okf_v01_legacy.md");
    assert!(legacy.contains("updated:"));
    assert!(legacy.contains("# Citations"));
    assert!(!legacy.contains("okf_version: \"0.2\""));
}
