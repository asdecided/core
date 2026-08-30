//! Artifact inspection (`decided.services.inspect`): classify one document and
//! report its structure, or aggregate types across a directory.
//!
//! Section names are stored normalized (e.g. `"success metrics"`); the
//! renderers format them (`.title()` for humans, snake_case for JSON).
//! Decision metadata (`status`, `category`, `supersedes`) is attached only
//! for decisions; relationships are spec-driven and exclude `supersedes`
//! (the documented v0.4.2/ADR-007 top-level-scalar exception).

use crate::classify::classify;
use crate::parse::{parse_file, Artifact};
use crate::pycompat::py_strip;
use crate::relationships::{extract_relationships, CorpusItem};
use crate::spec::{canonical_value, spec_for, specs};
use crate::walk::find_markdown_files;

/// Typed single-file inspection result (`InspectionResult`).
pub struct InspectionResult {
    /// Artifact name, or `"unknown"`.
    pub artifact_type: String,
    /// 0.0 – 1.0, already rounded to 2dp by `classify`.
    pub confidence: f64,
    pub present_sections: Vec<String>,
    pub missing_sections: Vec<String>,
    /// Decision metadata — populated only for decisions that declare it.
    pub status: Option<String>,
    pub category: Option<String>,
    /// Top-level scalar (v0.4.2 / ADR-007 exception), never in `relationships`.
    pub supersedes: Option<String>,
    /// `{snake_section -> [refs]}` in spec.optional order; `related_*` only.
    pub relationships: Vec<(String, Vec<String>)>,
}

/// One file's result inside a directory inspection.
pub struct FileInspection {
    pub path: String,
    pub artifact_type: String,
    pub confidence: f64,
}

/// Aggregated inspection across a directory of Markdown files.
pub struct DirectoryInspection {
    pub directory: String,
    pub recursive: bool,
    pub files: Vec<FileInspection>,
}

impl DirectoryInspection {
    pub fn total_files(&self) -> usize {
        self.files.len()
    }

    /// Known types first (in ARTIFACT_SPECS order), then `unknown`.
    pub fn counts(&self) -> Vec<(&str, usize)> {
        let mut counts: Vec<(&str, usize)> = specs().iter().map(|s| (s.name.as_str(), 0)).collect();
        counts.push(("unknown", 0));
        for f in &self.files {
            if let Some(slot) = counts.iter_mut().find(|(name, _)| *name == f.artifact_type) {
                slot.1 += 1;
            } else {
                // `counts.get(f.type, 0) + 1` — an unregistered type appends.
                counts.push((f.artifact_type.as_str(), 1));
            }
        }
        counts
    }

    pub fn unknown_count(&self) -> usize {
        self.counts()
            .iter()
            .find(|(name, _)| *name == "unknown")
            .map(|(_, n)| *n)
            .unwrap_or(0)
    }
}

/// `_first_line(body)` — the first non-empty line of a section body.
fn first_line(body: &str) -> &str {
    for line in crate::pycompat::py_splitlines(body) {
        let stripped = py_strip(line);
        if !stripped.is_empty() {
            return stripped;
        }
    }
    ""
}

/// `_attach_decision_metadata(result, product)`.
fn attach_decision_metadata(result: &mut InspectionResult, artifact: &Artifact) {
    let Some(spec) = spec_for("decision") else {
        return;
    };
    for (field_name, allowed) in &spec.metadata {
        let Some(body) = artifact.section(field_name) else {
            continue;
        };
        if body.is_empty() {
            continue;
        }
        let value = canonical_value(first_line(body), allowed);
        match field_name.as_str() {
            "status" => result.status = Some(value),
            "category" => result.category = Some(value),
            _ => {}
        }
    }
    if let Some(supersedes) = artifact.section("supersedes") {
        if !supersedes.is_empty() {
            // Metadata only (REQ-003): no validation, just normalize the value.
            result.supersedes = Some(first_line(supersedes).to_string());
        }
    }
}

/// `build_inspection(product)` — classify, then attach decision metadata and
/// relationships.
pub fn build_inspection(artifact: &Artifact) -> InspectionResult {
    let c = classify(artifact);
    let mut result = InspectionResult {
        artifact_type: c.artifact_type.clone(),
        confidence: c.confidence,
        present_sections: c.present_sections,
        missing_sections: c.missing_sections,
        status: None,
        category: None,
        supersedes: None,
        relationships: Vec::new(),
    };
    if c.artifact_type == "decision" {
        attach_decision_metadata(&mut result, artifact);
    }
    // Relationship metadata is spec-driven, so it applies to any recognized
    // type (Unknown has no spec and therefore no relationships).
    if let Some(spec) = spec_for(&c.artifact_type) {
        result.relationships = extract_relationships(artifact, spec);
    }
    result
}

/// `inspect_directory(directory, recursive)` — walk, classify, aggregate.
pub fn inspect_directory(directory: &str, recursive: bool) -> DirectoryInspection {
    use rayon::prelude::*;
    let files: Vec<FileInspection> = find_markdown_files(directory, recursive)
        .into_par_iter()
        .map(|entry| {
            let artifact = parse_file(&entry.display);
            let c = classify(&artifact);
            FileInspection {
                path: entry.display,
                artifact_type: c.artifact_type,
                confidence: c.confidence,
            }
        })
        .collect();
    DirectoryInspection {
        directory: directory.to_string(),
        recursive,
        files,
    }
}

/// Inspect an already-verified writable projection without reopening its
/// files. Graph federation uses this entry point so inherited materialisation
/// paths never leak into a root-local directory inspection.
pub fn inspect_directory_from_items(
    directory: &str,
    recursive: bool,
    items: &[CorpusItem],
) -> DirectoryInspection {
    use rayon::prelude::*;
    let files = items
        .par_iter()
        .filter(|item| recursive || !item.artifact_path.relative_path.contains('/'))
        .map(|item| {
            let classification = classify(&item.artifact);
            FileInspection {
                path: item.path.clone(),
                artifact_type: classification.artifact_type,
                confidence: classification.confidence,
            }
        })
        .collect();
    DirectoryInspection {
        directory: directory.to_string(),
        recursive,
        files,
    }
}
