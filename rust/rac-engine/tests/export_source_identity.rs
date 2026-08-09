use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use rac_engine::export::{
    build_corpus_export, build_documents_export, build_graph_export,
};
use rac_engine::output::{render_documents_jsonl, render_export_json, render_graph_json};
use serde_json::Value;

static NEXT_DIR: AtomicUsize = AtomicUsize::new(0);

struct Scratch(PathBuf);

impl Scratch {
    fn new(label: &str) -> Self {
        let suffix = NEXT_DIR.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "asdecided-source-{label}-{}-{suffix}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("decisions")).unwrap();
        fs::write(
            root.join("decisions/decision.md"),
            "---\nschema_version: 1\nid: APP-111111111111\ntype: decision\n---\n# Decision\n\n## Context\n\nA source fixture.\n\n## Decision\n\nKeep provenance stable.\n\n## Consequences\n\nExports agree.\n\n## Status\n\nAccepted\n",
        )
        .unwrap();
        Self(root)
    }

    fn corpus(&self) -> String {
        self.0.join("decisions").to_string_lossy().into_owned()
    }

    fn write_config(&self, repository_key: Option<&str>, source: Option<&str>) {
        fs::create_dir_all(self.0.join(".decided")).unwrap();
        let mut config = String::new();
        if let Some(key) = repository_key {
            config.push_str(&format!("repository_key: {key}\n"));
        }
        if let Some(source) = source {
            config.push_str(&format!("corpus:\n  source: {source}\n"));
        }
        fs::write(self.0.join(".decided/config.yaml"), config).unwrap();
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn projection_sources(corpus: &str) -> (String, String, String, String) {
    let viewer = build_corpus_export(corpus, "test".to_string()).unwrap();
    let viewer: Value = serde_json::from_str(&render_export_json(&viewer)).unwrap();

    let documents = build_documents_export(corpus).unwrap();
    let documents: Value = serde_json::from_str(&render_documents_jsonl(&documents)).unwrap();

    let graph = build_graph_export(corpus).unwrap();
    let graph: Value = serde_json::from_str(&render_graph_json(&graph)).unwrap();

    (
        viewer["corpus"]["name"].as_str().unwrap().to_string(),
        viewer["corpus"]["source"].as_str().unwrap().to_string(),
        documents["metadata"]["source"].as_str().unwrap().to_string(),
        graph["source"].as_str().unwrap().to_string(),
    )
}

#[test]
fn explicit_source_is_exact_and_shared_without_changing_name() {
    let repo = Scratch::new("explicit");
    repo.write_config(Some("APP"), Some("acme/payments-service"));

    assert_eq!(
        projection_sources(&repo.corpus()),
        (
            "decisions".to_string(),
            "acme/payments-service".to_string(),
            "acme/payments-service".to_string(),
            "acme/payments-service".to_string(),
        )
    );
}

#[test]
fn repository_key_and_basename_fallbacks_are_deterministic() {
    let initialised = Scratch::new("key-fallback");
    initialised.write_config(Some("APP"), None);
    let (_, viewer, documents, graph) = projection_sources(&initialised.corpus());
    assert_eq!((viewer.as_str(), documents.as_str(), graph.as_str()), ("app", "app", "app"));

    let uninitialised = Scratch::new("basename-fallback");
    let (_, viewer, documents, graph) = projection_sources(&uninitialised.corpus());
    assert_eq!(
        (viewer.as_str(), documents.as_str(), graph.as_str()),
        ("decisions", "decisions", "decisions")
    );
}

#[test]
fn equivalent_corpus_spellings_emit_identical_bytes() {
    let repo = Scratch::new("spelling");
    repo.write_config(Some("APP"), Some("acme/spelling"));
    let corpus = repo.corpus();
    let dotted = format!("{corpus}/./");

    let viewer = render_export_json(&build_corpus_export(&corpus, "test".to_string()).unwrap());
    let dotted_viewer =
        render_export_json(&build_corpus_export(&dotted, "test".to_string()).unwrap());
    assert_eq!(viewer, dotted_viewer);

    let documents = render_documents_jsonl(&build_documents_export(&corpus).unwrap());
    let dotted_documents = render_documents_jsonl(&build_documents_export(&dotted).unwrap());
    assert_eq!(documents, dotted_documents);

    let graph = render_graph_json(&build_graph_export(&corpus).unwrap());
    let dotted_graph = render_graph_json(&build_graph_export(&dotted).unwrap());
    assert_eq!(graph, dotted_graph);
}

#[test]
fn repeated_repository_keys_are_distinguished_by_explicit_source() {
    let first = Scratch::new("first-corpus");
    first.write_config(Some("APP"), Some("acme/first"));
    let second = Scratch::new("second-corpus");
    second.write_config(Some("APP"), Some("acme/second"));

    let first_export = build_documents_export(&first.corpus()).unwrap();
    let second_export = build_documents_export(&second.corpus()).unwrap();
    assert_eq!(first_export.documents[0].id, second_export.documents[0].id);
    assert_ne!(
        (&first_export.corpus_source, &first_export.documents[0].id),
        (&second_export.corpus_source, &second_export.documents[0].id)
    );
}

#[test]
fn invalid_explicit_source_is_not_silently_replaced() {
    let repo = Scratch::new("invalid");
    repo.write_config(Some("APP"), Some("Acme Payments"));

    let error = match build_graph_export(&repo.corpus()) {
        Ok(_) => panic!("invalid source unexpectedly exported"),
        Err(error) => error,
    };
    assert!(error.message().contains("invalid corpus.source"));
    assert!(error.message().contains("lower-case slash-namespaced"));
}

#[test]
fn source_only_config_is_valid_for_read_only_exports() {
    let repo = Scratch::new("source-only");
    repo.write_config(None, Some("acme/source-only"));
    let (_, viewer, documents, graph) = projection_sources(&repo.corpus());
    assert_eq!(
        (viewer.as_str(), documents.as_str(), graph.as_str()),
        ("acme/source-only", "acme/source-only", "acme/source-only")
    );
}
