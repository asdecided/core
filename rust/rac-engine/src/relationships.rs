//! Relationship extraction and validation (`decided.services.references`,
//! `decided.services.relationships`, `decided.core.relationship_types`), per
//! PORT-CONTRACT.d/05.
//!
//! Covers the surfaces the validate/corpus parity gate needs:
//! `validate_relationships` (directory), `validate_relationships_file`, and
//! `validate_document_against_corpus` (the `decided validate - --corpus` seam).

use std::collections::HashMap;
use std::path::PathBuf;

use crate::classify::classify;
use crate::corpus::{
    compatible_local_layer, ArtifactKey, ArtifactOrigin, ArtifactPath,
    PhysicalArtifactLocator, PhysicalCorpusLocator,
};
use crate::identity::{artifact_identifier, artifact_identifiers, strip_list_marker};
use crate::parse::{parse_file, Artifact};
use crate::pycompat::{py_casefold, py_splitlines, py_strip};
use crate::spec::{snake, spec_for, ArtifactSpec, RELATIONSHIP_SECTIONS};
use crate::validate::repository_root;
use crate::walk::find_markdown_files;

// Stable issue codes (JSON contract).
pub const ISSUE_DUPLICATE_IDENTIFIER: &str = "duplicate-artifact-identifier";
pub const ISSUE_TARGET_NOT_FOUND: &str = "relationship-target-not-found";
pub const ISSUE_TARGET_AMBIGUOUS: &str = "relationship-target-ambiguous";
pub const ISSUE_SELF_REFERENCE: &str = "relationship-self-reference";
pub const ISSUE_EDGE_UNSUPPORTED: &str = "relationship-edge-unsupported";
pub const ISSUE_TARGET_SUPERSEDED: &str = "relationship-target-superseded";
pub const ISSUE_TARGET_TYPE_MISMATCH: &str = "relationship-target-type-mismatch";
pub const ISSUE_RELATIONSHIP_CYCLE: &str = "relationship-cycle";
pub const ISSUE_SCOPE_TARGET_NOT_FOUND: &str = "applies-to-target-not-found";

/// Canonical intrinsic severity per finding (`RELATIONSHIP_SEVERITY`).
pub fn relationship_severity(code: &str) -> &'static str {
    match code {
        ISSUE_TARGET_NOT_FOUND
        | ISSUE_TARGET_AMBIGUOUS
        | ISSUE_TARGET_TYPE_MISMATCH
        | ISSUE_RELATIONSHIP_CYCLE
        | ISSUE_DUPLICATE_IDENTIFIER
        | ISSUE_SCOPE_TARGET_NOT_FOUND => "error",
        ISSUE_TARGET_SUPERSEDED | ISSUE_SELF_REFERENCE | ISSUE_EDGE_UNSUPPORTED => "warning",
        _ => "warning",
    }
}

// ---------------------------------------------------------------------------
// Edge registry (decided.core.relationship_types)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct EdgeSpec {
    pub name: &'static str,
    pub range: &'static [&'static str],
    pub acyclic: bool,
    pub forbids_target_status: bool,
    pub external: bool,
    pub filesystem_scoped: bool,
    /// `supersedes`/`verified_by`/`applies_to` are directional; `related_*` and
    /// `related_tickets` are not.
    pub directional: bool,
    /// Whether an external edge's target lives in the repository's configured
    /// ticketing provider (`related_tickets` only, ADR-088).
    pub external_provider: bool,
}

/// `edge_spec(name)` over the built-in registry.
pub fn edge_spec(name: &str) -> Option<&'static EdgeSpec> {
    static REGISTRY: [EdgeSpec; 9] = [
        EdgeSpec {
            name: "related_requirements",
            range: &["requirement"],
            acyclic: false,
            forbids_target_status: true,
            external: false,
            filesystem_scoped: false,
            directional: false,
            external_provider: false,
        },
        EdgeSpec {
            name: "related_decisions",
            range: &["decision"],
            acyclic: false,
            forbids_target_status: true,
            external: false,
            filesystem_scoped: false,
            directional: false,
            external_provider: false,
        },
        EdgeSpec {
            name: "related_roadmaps",
            range: &["roadmap"],
            acyclic: false,
            forbids_target_status: true,
            external: false,
            filesystem_scoped: false,
            directional: false,
            external_provider: false,
        },
        EdgeSpec {
            name: "related_prompts",
            range: &["prompt"],
            acyclic: false,
            forbids_target_status: true,
            external: false,
            filesystem_scoped: false,
            directional: false,
            external_provider: false,
        },
        EdgeSpec {
            name: "related_designs",
            range: &["design"],
            acyclic: false,
            forbids_target_status: true,
            external: false,
            filesystem_scoped: false,
            directional: false,
            external_provider: false,
        },
        EdgeSpec {
            name: "supersedes",
            range: &["decision"],
            acyclic: true,
            forbids_target_status: false,
            external: false,
            filesystem_scoped: false,
            directional: true,
            external_provider: false,
        },
        EdgeSpec {
            name: "related_tickets",
            range: &[],
            acyclic: false,
            forbids_target_status: true,
            external: true,
            filesystem_scoped: false,
            directional: false,
            external_provider: true,
        },
        EdgeSpec {
            name: "verified_by",
            range: &[],
            acyclic: false,
            forbids_target_status: true,
            external: true,
            filesystem_scoped: false,
            directional: true,
            external_provider: false,
        },
        EdgeSpec {
            name: "applies_to",
            range: &[],
            acyclic: false,
            forbids_target_status: true,
            external: true,
            filesystem_scoped: true,
            directional: true,
            external_provider: false,
        },
    ];
    REGISTRY.iter().find(|e| e.name == name)
}

// ---------------------------------------------------------------------------
// Reference extraction (decided.services.references)
// ---------------------------------------------------------------------------

/// `parse_references(body)`: one reference per non-empty line, one leading
/// well-formed list marker stripped.
pub fn parse_references(body: &str) -> Vec<String> {
    let mut refs = Vec::new();
    for line in py_splitlines(body) {
        let stripped = py_strip(line);
        if stripped.is_empty() {
            continue;
        }
        refs.push(py_strip(strip_list_marker(stripped)).to_string());
    }
    refs
}

/// `extract_relationships_full(product, spec)`: `{snake_section -> refs}` in
/// `spec.optional` order, including `supersedes`.
pub fn extract_relationships_full(
    artifact: &Artifact,
    spec: &ArtifactSpec,
) -> Vec<(String, Vec<String>)> {
    collect_relationships(artifact, spec, true)
}

/// `extract_relationships(product, spec)` — the `decided inspect` extractor:
/// identical to the full variant except `supersedes` is excluded (it stays a
/// top-level scalar in inspect output, ADR-007).
pub fn extract_relationships(
    artifact: &Artifact,
    spec: &ArtifactSpec,
) -> Vec<(String, Vec<String>)> {
    collect_relationships(artifact, spec, false)
}

/// `_collect(product, spec, allowed)` — the single core behind the two
/// extractors: `{snake_section -> refs}` in `spec.optional` order, only
/// sections present with at least one parsed reference.
fn collect_relationships(
    artifact: &Artifact,
    spec: &ArtifactSpec,
    include_supersedes: bool,
) -> Vec<(String, Vec<String>)> {
    let mut out = Vec::new();
    for section in &spec.optional {
        if !RELATIONSHIP_SECTIONS.iter().any(|(name, _)| name == section) {
            continue;
        }
        if !include_supersedes && section == "supersedes" {
            continue;
        }
        let Some(body) = artifact.section(section) else {
            continue;
        };
        if body.is_empty() {
            continue;
        }
        let refs = parse_references(body);
        if !refs.is_empty() {
            out.push((snake(section), refs));
        }
    }
    out
}

/// `unsupported_relationship_sections(product, spec)`: canonical-order
/// relationship sections declared with refs but absent from `spec.optional`.
pub fn unsupported_relationship_sections(artifact: &Artifact, spec: &ArtifactSpec) -> Vec<String> {
    let mut out = Vec::new();
    for (section, _) in RELATIONSHIP_SECTIONS.iter() {
        if spec.optional.iter().any(|s| s == section) {
            continue;
        }
        let Some(body) = artifact.section(section) else {
            continue;
        };
        if !body.is_empty() && !parse_references(body).is_empty() {
            out.push(section.to_string());
        }
    }
    out
}

/// `_is_retired_artifact(product, spec)`.
fn is_retired(artifact: &Artifact, spec: &ArtifactSpec) -> bool {
    if spec.retired_status.is_empty() {
        return false;
    }
    let Some(body) = artifact.section("status") else {
        return false;
    };
    if body.is_empty() {
        return false;
    }
    let ff = py_casefold(crate::pycompat::first_nonempty_line(body));
    spec.retired_status.iter().any(|s| py_casefold(s) == ff)
}

// ---------------------------------------------------------------------------
// Compact validation rows + resolution index (ADR-108)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct ValidationRow {
    pub key: ArtifactKey,
    pub artifact_path: ArtifactPath,
    pub origin: ArtifactOrigin,
    pub path: String,
    /// Artifact type name, or None for an Unknown/untyped document.
    pub spec_name: Option<String>,
    pub canonical_id: String,
    pub identifiers: Vec<String>,
    pub retired: bool,
    /// Canonical (space) section names.
    pub unsupported_sections: Vec<String>,
    /// `(snake_section, refs)` in schema order.
    pub edges: Vec<(String, Vec<String>)>,
}

pub fn validation_row(
    path: &str,
    artifact: &Artifact,
    spec: Option<&ArtifactSpec>,
) -> ValidationRow {
    let parent = std::path::Path::new(path)
        .parent()
        .and_then(std::path::Path::to_str)
        .unwrap_or(".");
    let layer = compatible_local_layer(parent);
    let origin = layer.origin();
    let relative_path = std::path::Path::new(path)
        .file_name()
        .and_then(std::ffi::OsStr::to_str)
        .unwrap_or(path);
    let identifiers = artifact_identifiers(artifact, spec, path);
    let canonical_id = artifact_identifier(artifact, spec, path);
    validation_row_with_identity(
        path,
        artifact,
        spec,
        origin.key(canonical_id),
        origin.path(relative_path),
        origin,
        identifiers,
    )
}

pub(crate) fn validation_row_from_item(item: &CorpusItem) -> ValidationRow {
    validation_row_with_identity(
        &item.path,
        &item.artifact,
        item.spec,
        item.key.clone(),
        item.artifact_path.clone(),
        item.origin.clone(),
        artifact_identifiers(&item.artifact, item.spec, &item.path),
    )
}

fn validation_row_with_identity(
    path: &str,
    artifact: &Artifact,
    spec: Option<&ArtifactSpec>,
    key: ArtifactKey,
    artifact_path: ArtifactPath,
    origin: ArtifactOrigin,
    identifiers: Vec<String>,
) -> ValidationRow {
    let canonical_id = key.canonical_id.clone();
    match spec {
        None => ValidationRow {
            key,
            artifact_path,
            origin,
            path: path.to_string(),
            spec_name: None,
            canonical_id,
            identifiers,
            retired: false,
            unsupported_sections: Vec::new(),
            edges: Vec::new(),
        },
        Some(spec) => ValidationRow {
            key,
            artifact_path,
            origin,
            path: path.to_string(),
            spec_name: Some(spec.name.clone()),
            canonical_id,
            identifiers,
            retired: is_retired(artifact, spec),
            unsupported_sections: unsupported_relationship_sections(artifact, spec),
            edges: extract_relationships_full(artifact, spec),
        },
    }
}

/// One source-aware resolution candidate.
///
/// `path` is retained solely as the released display value. Identity and
/// deterministic ordering use `key` and `artifact_path`, never a checkout
/// path (ADR-135/ADR-136).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolutionCandidate {
    pub key: ArtifactKey,
    pub artifact_path: ArtifactPath,
    pub path: String,
    pub identifier: String,
}

impl ResolutionCandidate {
    pub(crate) fn from_row(row: &ValidationRow, identifier: String) -> Self {
        Self {
            key: row.key.clone(),
            artifact_path: row.artifact_path.clone(),
            path: row.path.clone(),
            identifier,
        }
    }
}

/// Insertion-ordered `{casefold(ident) -> [source-aware candidate]}` index.
pub struct ResolutionIndex {
    order: Vec<String>,
    map: HashMap<String, Vec<ResolutionCandidate>>,
}

impl ResolutionIndex {
    pub(crate) fn new() -> Self {
        ResolutionIndex {
            order: Vec::new(),
            map: HashMap::new(),
        }
    }

    pub(crate) fn insert(&mut self, key: String, value: ResolutionCandidate) {
        match self.map.get_mut(&key) {
            Some(v) => {
                if !v.iter().any(|candidate| {
                    candidate.key == value.key && candidate.artifact_path == value.artifact_path
                }) {
                    v.push(value);
                    v.sort_by(|left, right| {
                        left.artifact_path
                            .cmp(&right.artifact_path)
                            .then_with(|| left.key.cmp(&right.key))
                    });
                }
            }
            None => {
                self.map.insert(key.clone(), vec![value]);
                self.order.push(key);
            }
        }
    }

    pub fn get(&self, key: &str) -> &[ResolutionCandidate] {
        self.map.get(key).map(|v| v.as_slice()).unwrap_or(&[])
    }

    pub(crate) fn reference_key(reference: &str) -> String {
        match reference.split_once("::") {
            Some((alias, canonical_id)) if !canonical_id.contains("::") => {
                format!("{alias}::{}", py_casefold(canonical_id))
            }
            _ => py_casefold(reference),
        }
    }

    pub(crate) fn get_reference(&self, reference: &str) -> &[ResolutionCandidate] {
        self.get(&Self::reference_key(reference))
    }

    fn values(&self) -> impl Iterator<Item = &Vec<ResolutionCandidate>> {
        self.order.iter().map(|k| &self.map[k])
    }
}

pub fn resolution_index_from_rows(rows: &[ValidationRow]) -> ResolutionIndex {
    let mut index = ResolutionIndex::new();
    for row in rows {
        for ident in &row.identifiers {
            index.insert(
                py_casefold(ident),
                ResolutionCandidate::from_row(row, ident.clone()),
            );
        }
    }
    index
}

// ---------------------------------------------------------------------------
// Findings model
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct RelationshipIssue {
    pub code: String,
    pub source_path: Option<String>,
    pub relationship: Option<String>,
    pub target: Option<String>,
    pub identifier: Option<String>,
    pub paths: Option<Vec<String>>,
}

impl RelationshipIssue {
    fn reference(code: &str, source_path: &str, relationship: &str, target: &str) -> Self {
        RelationshipIssue {
            code: code.to_string(),
            source_path: Some(source_path.to_string()),
            relationship: Some(relationship.to_string()),
            target: Some(target.to_string()),
            identifier: None,
            paths: None,
        }
    }
}

#[derive(Debug)]
pub struct RelationshipValidation {
    pub directory: String,
    pub recursive: bool,
    pub relationships_checked: usize,
    pub issues: Vec<RelationshipIssue>,
}

impl RelationshipValidation {
    pub fn ok(&self) -> bool {
        self.issues.is_empty()
    }
}

// ---------------------------------------------------------------------------
// Scope entries (decided.services.scope_paths)
// ---------------------------------------------------------------------------

/// `classify_scope_entry(entry)` -> "glob" | "path" | "component".
pub(crate) fn classify_scope_entry(entry: &str) -> &'static str {
    if entry.contains('*') || entry.contains('?') || entry.contains('[') {
        "glob"
    } else if entry.contains('/') {
        "path"
    } else {
        "component"
    }
}

/// `normalized_scope_path(entry)` — POSIX repo-relative form, or None.
pub(crate) fn normalized_scope_path(entry: &str) -> Option<String> {
    let text = py_strip(entry);
    if text.is_empty() || text.starts_with('/') {
        return None;
    }
    let mut parts: Vec<&str> = Vec::new();
    for part in text.split('/').filter(|p| !p.is_empty()) {
        if part == "." {
            continue;
        }
        if part == ".." {
            return None;
        }
        parts.push(part);
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join("/"))
    }
}

// ---------------------------------------------------------------------------
// validation_from_rows — the gate core
// ---------------------------------------------------------------------------

fn resolved_unique<'a>(
    index: &'a ResolutionIndex,
    reference: &str,
    source_key: &ArtifactKey,
) -> Option<&'a ResolutionCandidate> {
    let targets = index.get_reference(reference);
    if targets.len() != 1 || targets[0].key == *source_key {
        return None;
    }
    Some(&targets[0])
}

/// Outcome of resolving one internal reference against the index: checked
/// empty -> not found, then multiple -> ambiguous, then same-path -> self,
/// else uniquely resolved. Shared by the issue and `Relationship` loops.
enum ReferenceResolution<'a> {
    Resolved(&'a ResolutionCandidate),
    NotFound,
    Ambiguous,
    SelfRef,
}

fn classify_reference<'a>(
    index: &'a ResolutionIndex,
    reference: &str,
    source_key: &ArtifactKey,
) -> ReferenceResolution<'a> {
    let targets = index.get_reference(reference);
    if targets.is_empty() {
        ReferenceResolution::NotFound
    } else if targets.len() > 1 {
        ReferenceResolution::Ambiguous
    } else if targets[0].key == *source_key {
        ReferenceResolution::SelfRef
    } else {
        ReferenceResolution::Resolved(&targets[0])
    }
}

/// `_resolve_references(rows, index)` -> `(checked, issues)`.
fn resolve_references(
    rows: &[ValidationRow],
    index: &ResolutionIndex,
) -> (usize, Vec<RelationshipIssue>) {
    let (checked, issues, _) = resolve_references_full(rows, index);
    (checked, issues)
}

/// Tarjan SCC over source-aware keys. Traversal and output are ordered by the
/// corresponding stable `(source, relative_path)`, not display paths.
fn cyclic_components(
    adjacency: &[(ArtifactKey, Vec<ArtifactKey>)],
    paths: &HashMap<ArtifactKey, ArtifactPath>,
) -> Vec<Vec<ArtifactKey>> {
    let adj: HashMap<ArtifactKey, Vec<ArtifactKey>> = adjacency.iter().cloned().collect();
    let mut nodes: Vec<ArtifactKey> = adjacency
        .iter()
        .flat_map(|(key, values)| std::iter::once(key.clone()).chain(values.iter().cloned()))
        .collect();
    nodes.sort_by(|left, right| paths[left].cmp(&paths[right]).then_with(|| left.cmp(right)));
    nodes.dedup();

    struct State {
        indices: HashMap<ArtifactKey, usize>,
        lowlink: HashMap<ArtifactKey, usize>,
        on_stack: std::collections::HashSet<ArtifactKey>,
        stack: Vec<ArtifactKey>,
        counter: usize,
        components: Vec<Vec<ArtifactKey>>,
    }

    fn strongconnect(
        v: &ArtifactKey,
        adj: &HashMap<ArtifactKey, Vec<ArtifactKey>>,
        paths: &HashMap<ArtifactKey, ArtifactPath>,
        st: &mut State,
    ) {
        st.indices.insert(v.clone(), st.counter);
        st.lowlink.insert(v.clone(), st.counter);
        st.counter += 1;
        st.stack.push(v.clone());
        st.on_stack.insert(v.clone());
        if let Some(neighbors) = adj.get(v) {
            for w in neighbors {
                if !st.indices.contains_key(w) {
                    strongconnect(w, adj, paths, st);
                    let lw = st.lowlink[w];
                    let lv = st.lowlink[v];
                    st.lowlink.insert(v.clone(), lv.min(lw));
                } else if st.on_stack.contains(w) {
                    let iw = st.indices[w];
                    let lv = st.lowlink[v];
                    st.lowlink.insert(v.clone(), lv.min(iw));
                }
            }
        }
        if st.lowlink[v] == st.indices[v] {
            let mut component: Vec<ArtifactKey> = Vec::new();
            loop {
                let w = st.stack.pop().expect("stack nonempty");
                st.on_stack.remove(&w);
                let complete = w == *v;
                component.push(w);
                if complete {
                    break;
                }
            }
            if component.len() > 1 {
                component.sort_by(|left, right| {
                    paths[left].cmp(&paths[right]).then_with(|| left.cmp(right))
                });
                st.components.push(component);
            }
        }
    }

    let mut st = State {
        indices: HashMap::new(),
        lowlink: HashMap::new(),
        on_stack: std::collections::HashSet::new(),
        stack: Vec::new(),
        counter: 0,
        components: Vec::new(),
    };
    for node in &nodes {
        if !st.indices.contains_key(node) {
            strongconnect(node, &adj, paths, &mut st);
        }
    }
    st.components.sort_by(|left, right| {
        paths[&left[0]]
            .cmp(&paths[&right[0]])
            .then_with(|| left[0].cmp(&right[0]))
    });
    st.components
}

fn cycle_issues(
    rows: &[ValidationRow],
    target_rows: &[ValidationRow],
    index: &ResolutionIndex,
) -> Vec<RelationshipIssue> {
    let paths: HashMap<ArtifactKey, ArtifactPath> = target_rows
        .iter()
        .map(|row| (row.key.clone(), row.artifact_path.clone()))
        .collect();
    let display_paths: HashMap<&ArtifactKey, &str> = target_rows
        .iter()
        .map(|row| (&row.key, row.path.as_str()))
        .collect();
    // Sorted acyclic edge kinds — today only `supersedes`.
    let mut issues = Vec::new();
    for kind in ["supersedes"] {
        // `_acyclic_adjacency`: {source -> sorted unique resolved non-self targets}.
        let mut adjacency: Vec<(ArtifactKey, Vec<ArtifactKey>)> = Vec::new();
        for row in rows {
            if row.spec_name.is_none() {
                continue;
            }
            let refs = row
                .edges
                .iter()
                .find(|(s, _)| s == kind)
                .map(|(_, r)| r.as_slice())
                .unwrap_or(&[]);
            let mut targets: Vec<ArtifactKey> = Vec::new();
            for reference in refs {
                if let Some(target) = resolved_unique(index, reference, &row.key) {
                    if !targets.iter().any(|key| key == &target.key) {
                        targets.push(target.key.clone());
                    }
                }
            }
            if !targets.is_empty() {
                targets.sort_by(|left, right| {
                    paths[left].cmp(&paths[right]).then_with(|| left.cmp(right))
                });
                adjacency.push((row.key.clone(), targets));
            }
        }
        for component in cyclic_components(&adjacency, &paths) {
            issues.push(RelationshipIssue {
                code: ISSUE_RELATIONSHIP_CYCLE.to_string(),
                source_path: None,
                relationship: Some(kind.to_string()),
                target: None,
                identifier: None,
                paths: Some(
                    component
                        .iter()
                        .map(|key| display_paths[key].to_string())
                        .collect(),
                ),
            });
        }
    }
    issues
}

fn scope_validation_issues(directory: &str, rows: &[ValidationRow]) -> Vec<RelationshipIssue> {
    let root: PathBuf = repository_root(directory);
    let mut issues = Vec::new();
    for row in rows {
        if row.spec_name.is_none() {
            continue;
        }
        for (section, refs) in &row.edges {
            let Some(edge) = edge_spec(section) else {
                continue;
            };
            if !edge.filesystem_scoped {
                continue;
            }
            for reference in refs {
                if classify_scope_entry(reference) != "path" {
                    continue;
                }
                if let Some(normalized) = normalized_scope_path(reference) {
                    if root.join(&normalized).exists() {
                        continue;
                    }
                }
                issues.push(RelationshipIssue::reference(
                    ISSUE_SCOPE_TARGET_NOT_FOUND,
                    &row.path,
                    section,
                    reference,
                ));
            }
        }
    }
    issues
}

/// `validation_from_rows(directory, rows, recursive)` — the single gate core.
pub fn validation_from_rows(
    directory: &str,
    rows: &[ValidationRow],
    recursive: bool,
) -> RelationshipValidation {
    let index = resolution_index_from_rows(rows);
    validation_from_rows_with_index(directory, rows, rows, recursive, &index, true)
}

/// Source-aware validation core used by the composed read model. The caller
/// supplies the one resolution index so qualified references and override
/// redirects cannot diverge between graph construction and validation.
pub(crate) fn validation_from_rows_with_index(
    directory: &str,
    rows: &[ValidationRow],
    target_rows: &[ValidationRow],
    recursive: bool,
    index: &ResolutionIndex,
    include_duplicate_identifiers: bool,
) -> RelationshipValidation {
    let mut issues: Vec<RelationshipIssue> = Vec::new();

    // Duplicate identifiers first, sorted by display identifier (casefold).
    if include_duplicate_identifiers {
        let mut ident_index = ResolutionIndex::new();
        for row in rows {
            ident_index.insert(
                py_casefold(&row.canonical_id),
                ResolutionCandidate::from_row(row, row.canonical_id.clone()),
            );
        }
        let mut duplicates: Vec<(String, Vec<String>)> = Vec::new();
        for entries in ident_index.values() {
            if entries.len() > 1 {
                let display = entries
                    .iter()
                    .min_by(|left, right| left.artifact_path.cmp(&right.artifact_path))
                    .expect("nonempty")
                    .identifier
                    .clone();
                let mut paths: Vec<&ResolutionCandidate> = entries.iter().collect();
                paths.sort_by(|left, right| {
                    left.artifact_path
                        .cmp(&right.artifact_path)
                        .then_with(|| left.key.cmp(&right.key))
                });
                duplicates.push((
                    display,
                    paths.into_iter().map(|entry| entry.path.clone()).collect(),
                ));
            }
        }
        duplicates.sort_by_cached_key(|entry| py_casefold(&entry.0));
        for (display, dup_paths) in duplicates {
            issues.push(RelationshipIssue {
                code: ISSUE_DUPLICATE_IDENTIFIER.to_string(),
                source_path: None,
                relationship: None,
                target: None,
                identifier: Some(display),
                paths: Some(dup_paths),
            });
        }
    }

    // Edge-legality: unsupported declared sections (canonical order per row).
    for row in rows {
        if row.spec_name.is_none() {
            continue;
        }
        for section in &row.unsupported_sections {
            issues.push(RelationshipIssue {
                code: ISSUE_EDGE_UNSUPPORTED.to_string(),
                source_path: Some(row.path.clone()),
                relationship: Some(snake(section)),
                target: None,
                identifier: None,
                paths: None,
            });
        }
    }

    let by_key: HashMap<&ArtifactKey, &ValidationRow> =
        target_rows.iter().map(|row| (&row.key, row)).collect();

    // Range violations.
    for row in rows {
        if row.spec_name.is_none() {
            continue;
        }
        for (section, refs) in &row.edges {
            let Some(edge) = edge_spec(section) else {
                continue;
            };
            if edge.external {
                continue;
            }
            for reference in refs {
                let Some(target) = resolved_unique(index, reference, &row.key) else {
                    continue;
                };
                let Some(target_row) = by_key.get(&target.key) else {
                    continue;
                };
                let Some(target_spec) = target_row.spec_name.as_deref() else {
                    continue;
                };
                if !edge.range.contains(&target_spec) {
                    issues.push(RelationshipIssue::reference(
                        ISSUE_TARGET_TYPE_MISMATCH,
                        &row.path,
                        section,
                        reference,
                    ));
                }
            }
        }
    }

    // Status-consistency: live source -> retired target.
    for row in rows {
        if row.spec_name.is_none() || row.retired {
            continue;
        }
        for (section, refs) in &row.edges {
            let Some(edge) = edge_spec(section) else {
                continue;
            };
            if edge.external || !edge.forbids_target_status {
                continue;
            }
            for reference in refs {
                let Some(target) = resolved_unique(index, reference, &row.key) else {
                    continue;
                };
                if by_key
                    .get(&target.key)
                    .is_some_and(|target_row| target_row.retired)
                {
                    issues.push(RelationshipIssue::reference(
                        ISSUE_TARGET_SUPERSEDED,
                        &row.path,
                        section,
                        reference,
                    ));
                }
            }
        }
    }

    // Acyclicity.
    issues.extend(cycle_issues(rows, target_rows, index));

    // Referential integrity.
    let (checked, ref_issues) = resolve_references(rows, index);
    issues.extend(ref_issues);

    // Code-scope existence (appended last).
    issues.extend(scope_validation_issues(directory, rows));

    RelationshipValidation {
        directory: directory.to_string(),
        recursive,
        relationships_checked: checked,
        issues,
    }
}

// ---------------------------------------------------------------------------
// Repository-level relationship inspection (non-validate report)
// ---------------------------------------------------------------------------

/// One artifact's relationships in a report (`ArtifactRelationships`).
#[derive(Debug, Clone)]
pub struct ArtifactRelationships {
    pub path: String,
    pub type_name: String,
    /// `(snake_section, refs)` in `spec.optional` order.
    pub relationships: Vec<(String, Vec<String>)>,
}

/// `RelationshipReport` (non-validate inspection).
#[derive(Debug)]
pub struct RelationshipReport {
    pub directory: String,
    pub recursive: bool,
    pub total_files: usize,
    pub artifacts: Vec<ArtifactRelationships>,
    /// `{casefold(ref) -> "Title (type · id)"}` for uniquely-resolved refs.
    /// Insertion-ordered (first-seen wins); presentation-only, never in JSON.
    pub labels: HashMap<String, String>,
}

impl RelationshipReport {
    pub fn artifacts_with_relationships(&self) -> usize {
        self.artifacts.len()
    }

    /// References per relationship type, canonical order, zero types omitted.
    pub fn counts(&self) -> Vec<(String, usize)> {
        let mut totals: HashMap<String, usize> = HashMap::new();
        for artifact in &self.artifacts {
            for (section, refs) in &artifact.relationships {
                *totals.entry(section.clone()).or_insert(0) += refs.len();
            }
        }
        let mut out = Vec::new();
        for (_, snake_key) in crate::spec::RELATIONSHIP_SECTIONS.iter() {
            if let Some(count) = totals.get(*snake_key) {
                out.push((snake_key.to_string(), *count));
            }
        }
        out
    }

    pub fn relationship_count(&self) -> usize {
        self.counts().iter().map(|(_, c)| c).sum()
    }
}

/// `_resolution_labels(artifacts, items)`.
fn resolution_labels(
    artifacts: &[ArtifactRelationships],
    items: &[CorpusItem],
) -> HashMap<String, String> {
    // Resolution index over every alias of every item, in item order.
    let mut index = ResolutionIndex::new();
    let mut info: HashMap<&ArtifactKey, (String, Option<&'static ArtifactSpec>, Option<String>)> =
        HashMap::new();
    for item in items {
        let identifiers = artifact_identifiers(&item.artifact, item.spec, &item.path);
        for ident in &identifiers {
            index.insert(
                py_casefold(ident),
                ResolutionCandidate {
                    key: item.key.clone(),
                    artifact_path: item.artifact_path.clone(),
                    path: item.path.clone(),
                    identifier: ident.clone(),
                },
            );
        }
        let canonical = artifact_identifier(&item.artifact, item.spec, &item.path);
        info.insert(
            &item.key,
            (canonical, item.spec, item.artifact.product.title.clone()),
        );
    }
    let mut labels: HashMap<String, String> = HashMap::new();
    for artifact in artifacts {
        for (_, refs) in &artifact.relationships {
            for reference in refs {
                let key = py_casefold(reference);
                if labels.contains_key(&key) {
                    continue;
                }
                let entries = index.get(&key);
                let mut distinct: Vec<&ArtifactKey> =
                    entries.iter().map(|entry| &entry.key).collect();
                distinct.sort();
                distinct.dedup();
                if distinct.len() != 1 {
                    continue;
                }
                let (canonical, spec, title) = &info[distinct[0]];
                let type_name = spec.map(|s| s.name.as_str()).unwrap_or("unknown");
                let display = match title {
                    Some(t) if !t.is_empty() => t.as_str(),
                    _ => canonical.as_str(),
                };
                labels.insert(key, format!("{display} ({type_name} · {canonical})"));
            }
        }
    }
    labels
}

/// `_build_report(directory, items, recursive)`.
fn build_report(directory: &str, items: Vec<CorpusItem>, recursive: bool) -> RelationshipReport {
    let mut artifacts: Vec<ArtifactRelationships> = Vec::new();
    for item in &items {
        let Some(spec) = item.spec else {
            continue;
        };
        let relationships = extract_relationships_full(&item.artifact, spec);
        if !relationships.is_empty() {
            artifacts.push(ArtifactRelationships {
                path: item.path.clone(),
                type_name: spec.name.clone(),
                relationships,
            });
        }
    }
    let labels = resolution_labels(&artifacts, &items);
    RelationshipReport {
        directory: directory.to_string(),
        recursive,
        total_files: items.len(),
        artifacts,
        labels,
    }
}

pub fn build_relationship_report(directory: &str, recursive: bool) -> RelationshipReport {
    build_report(directory, corpus_items(directory, recursive), recursive)
}

pub fn build_relationship_report_file(path: &str) -> RelationshipReport {
    let artifact = parse_file(path);
    let spec = spec_for(&classify(&artifact).artifact_type);
    let items = vec![CorpusItem::compatible_local_file(path, artifact, spec)];
    build_report(path, items, false)
}

// ---------------------------------------------------------------------------
// Corpus entry points
// ---------------------------------------------------------------------------

/// One parsed + classified item with stable identity and a separate runtime
/// locator. `path` remains the released display string.
#[derive(Clone)]
pub struct CorpusItem {
    pub key: ArtifactKey,
    pub artifact_path: ArtifactPath,
    pub origin: ArtifactOrigin,
    pub locator: PhysicalArtifactLocator,
    pub path: String,
    pub artifact: Artifact,
    pub spec: Option<&'static ArtifactSpec>,
}

impl CorpusItem {
    pub fn new(
        path: String,
        relative_path: String,
        artifact: Artifact,
        spec: Option<&'static ArtifactSpec>,
        origin: ArtifactOrigin,
        locator: PhysicalArtifactLocator,
    ) -> Self {
        let canonical_id = artifact_identifier(&artifact, spec, &path);
        Self {
            key: origin.key(canonical_id),
            artifact_path: origin.path(relative_path),
            origin,
            locator,
            path,
            artifact,
            spec,
        }
    }

    /// Compatibility constructor for single-file and synthetic validation
    /// seams that do not begin with a corpus walk.
    pub fn compatible_local_file(
        path: &str,
        artifact: Artifact,
        spec: Option<&'static ArtifactSpec>,
    ) -> Self {
        let file = std::path::Path::new(path);
        let corpus_root = file.parent().unwrap_or_else(|| std::path::Path::new("."));
        let corpus_root_text = corpus_root.to_string_lossy();
        let origin = compatible_local_layer(&corpus_root_text).origin();
        let relative_path = file
            .file_name()
            .and_then(std::ffi::OsStr::to_str)
            .unwrap_or(path)
            .to_string();
        Self::new(
            path.to_string(),
            relative_path,
            artifact,
            spec,
            origin,
            PhysicalArtifactLocator::new(
                PhysicalCorpusLocator::local(corpus_root),
                file,
            ),
        )
    }
}

/// `_corpus_items(directory, recursive)` — the sorted-path walk, parsed and
/// classified.
///
/// The per-file parse+classify work is parallelized with rayon over the
/// already-sorted file list (PORT-CONTRACT decision 5). `into_par_iter` on a
/// `Vec` is an indexed parallel iterator, so `collect` reassembles results in
/// the original (sorted) order — the worker count is invisible in the output.
/// Rendering stays sequential over the ordered results.
pub fn corpus_items(directory: &str, recursive: bool) -> Vec<CorpusItem> {
    use rayon::prelude::*;
    let origin = compatible_local_layer(directory).origin();
    let physical_corpus = PhysicalCorpusLocator::local(directory);
    find_markdown_files(directory, recursive)
        .into_par_iter()
        .map(|entry| {
            let relative_path = entry.rel();
            let artifact = parse_file(&entry.display);
            let spec = spec_for(&classify(&artifact).artifact_type);
            CorpusItem::new(
                entry.display,
                relative_path,
                artifact,
                spec,
                origin.clone(),
                PhysicalArtifactLocator::new(physical_corpus.clone(), entry.abs),
            )
        })
        .collect()
}

fn rows_from_items(items: &[CorpusItem]) -> Vec<ValidationRow> {
    items
        .iter()
        .map(validation_row_from_item)
        .collect()
}

pub fn validate_relationships(directory: &str, recursive: bool) -> RelationshipValidation {
    let items = corpus_items(directory, recursive);
    validation_from_rows(directory, &rows_from_items(&items), recursive)
}

pub fn validate_relationships_file(path: &str) -> RelationshipValidation {
    let artifact = parse_file(path);
    let spec = spec_for(&classify(&artifact).artifact_type);
    let rows = vec![validation_row(path, &artifact, spec)];
    validation_from_rows(path, &rows, false)
}

/// `validate_document_against_corpus(product, source_path, directory)` — the
/// `decided validate - --corpus DIR` seam (ADR-067).
pub fn validate_document_against_corpus(
    artifact: &Artifact,
    source_path: &str,
    directory: &str,
    recursive: bool,
) -> RelationshipValidation {
    let corpus = corpus_items(directory, recursive);
    let spec = spec_for(&classify(artifact).artifact_type);
    let proposed_ident = py_casefold(&artifact_identifier(artifact, spec, source_path));
    let mut rows: Vec<ValidationRow> = Vec::new();
    for item in &corpus {
        let ident = artifact_identifier(&item.artifact, item.spec, &item.path);
        if py_casefold(&ident) == proposed_ident {
            continue; // the on-disk counterpart of the document being edited
        }
        rows.push(validation_row_from_item(item));
    }
    rows.push(validation_row(source_path, artifact, spec));
    let result = validation_from_rows(directory, &rows, recursive);
    let own: Vec<RelationshipIssue> = result
        .issues
        .into_iter()
        .filter(|i| i.source_path.as_deref() == Some(source_path))
        .collect();
    RelationshipValidation {
        directory: directory.to_string(),
        recursive,
        relationships_checked: result.relationships_checked,
        issues: own,
    }
}

// ---------------------------------------------------------------------------
// Resolved relationship objects (decided.services.relationships.Relationship)
// ---------------------------------------------------------------------------

/// One declared cross-artifact reference with its resolution outcome
/// (`Relationship`). `resolved_path` is set only when the reference resolves
/// uniquely to another artifact.
#[derive(Debug, Clone)]
pub struct Relationship {
    /// Stable source-aware endpoint. `None` only when reconstructed from the
    /// pre-federation v1 persistent store; the v2 cutover owns its codec.
    pub source_artifact: Option<ArtifactPath>,
    pub source_path: String,
    pub relationship: String,
    pub target: String,
    /// Stable resolved endpoint, present for a uniquely resolved internal
    /// edge built from the in-memory read model.
    pub resolved_artifact: Option<ArtifactPath>,
    pub resolved_path: Option<String>,
    pub issue: Option<String>,
}

/// `resolve_relationships(rows, index)` — the single resolve loop. Rows in
/// order, sections in each row's schema order, refs in declaration order.
pub fn resolve_relationships(
    rows: &[ValidationRow],
    index: &ResolutionIndex,
) -> Vec<Relationship> {
    let mut out = Vec::new();
    for row in rows {
        for (section, refs) in &row.edges {
            let external = edge_spec(section).is_some_and(|e| e.external);
            for reference in refs {
                if external {
                    out.push(Relationship {
                        source_artifact: Some(row.artifact_path.clone()),
                        source_path: row.path.clone(),
                        relationship: section.clone(),
                        target: reference.clone(),
                        resolved_artifact: None,
                        resolved_path: None,
                        issue: None,
                    });
                    continue;
                }
                let (resolved, resolved_artifact, issue) =
                    match classify_reference(index, reference, &row.key) {
                    ReferenceResolution::Resolved(target) => (
                        Some(target.path.clone()),
                        Some(target.artifact_path.clone()),
                        None,
                    ),
                    ReferenceResolution::NotFound => {
                        (None, None, Some(ISSUE_TARGET_NOT_FOUND.to_string()))
                    }
                    ReferenceResolution::Ambiguous => {
                        (None, None, Some(ISSUE_TARGET_AMBIGUOUS.to_string()))
                    }
                    ReferenceResolution::SelfRef => {
                        (None, None, Some(ISSUE_SELF_REFERENCE.to_string()))
                    }
                };
                out.push(Relationship {
                    source_artifact: Some(row.artifact_path.clone()),
                    source_path: row.path.clone(),
                    relationship: section.clone(),
                    target: reference.clone(),
                    resolved_artifact,
                    resolved_path: resolved,
                    issue,
                });
            }
        }
    }
    out
}

/// `relationships_from_corpus(entries)` — every declared reference in a corpus
/// snapshot, resolved. Ordering matches `_resolve_references`.
pub fn relationships_from_corpus(items: &[CorpusItem]) -> Vec<Relationship> {
    let rows = rows_from_items(items);
    let index = resolution_index_from_rows(&rows);
    resolve_relationships(&rows, &index)
}

// ---------------------------------------------------------------------------
// Relationship summary (decided.services.relationships.RelationshipSummary)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct RelationshipSummary {
    pub total: usize,
    pub valid: usize,
    pub broken: usize,
    pub orphaned: usize,
    pub coverage: f64,
    pub issues: Vec<RelationshipIssue>,
}

/// `_resolve_references(rows, index)` returning also the set of resolved target
/// paths (for orphan detection).
fn resolve_references_full(
    rows: &[ValidationRow],
    index: &ResolutionIndex,
) -> (
    usize,
    Vec<RelationshipIssue>,
    std::collections::HashSet<ArtifactKey>,
) {
    let mut issues = Vec::new();
    let mut resolved_targets: std::collections::HashSet<ArtifactKey> =
        std::collections::HashSet::new();
    let mut checked = 0usize;
    for row in rows {
        if row.spec_name.is_none() {
            continue;
        }
        for (section, refs) in &row.edges {
            if edge_spec(section).is_some_and(|e| e.external) {
                continue;
            }
            for reference in refs {
                checked += 1;
                let code = match classify_reference(index, reference, &row.key) {
                    ReferenceResolution::Resolved(target) => {
                        resolved_targets.insert(target.key.clone());
                        continue;
                    }
                    ReferenceResolution::NotFound => ISSUE_TARGET_NOT_FOUND,
                    ReferenceResolution::Ambiguous => ISSUE_TARGET_AMBIGUOUS,
                    ReferenceResolution::SelfRef => ISSUE_SELF_REFERENCE,
                };
                issues.push(RelationshipIssue::reference(code, &row.path, section, reference));
            }
        }
    }
    (checked, issues, resolved_targets)
}

/// `summary_from_rows(rows)`.
pub fn summary_from_rows(rows: &[ValidationRow]) -> RelationshipSummary {
    if rows.is_empty() {
        return RelationshipSummary {
            total: 0,
            valid: 0,
            broken: 0,
            orphaned: 0,
            coverage: 1.0,
            issues: Vec::new(),
        };
    }
    let index = resolution_index_from_rows(rows);
    let (checked, ref_issues, resolved_targets) = resolve_references_full(rows, &index);
    let broken = ref_issues.len();
    let valid = checked - broken;

    let known_keys: Vec<&ArtifactKey> = rows
        .iter()
        .filter(|r| r.spec_name.is_some())
        .map(|r| &r.key)
        .collect();
    let orphaned = known_keys
        .iter()
        .filter(|key| !resolved_targets.contains(**key))
        .count();
    let artifacts_with_rels = rows
        .iter()
        .filter(|r| r.spec_name.is_some() && !r.edges.is_empty())
        .count();
    let coverage = if known_keys.is_empty() {
        1.0
    } else {
        crate::pycompat::py_round(artifacts_with_rels as f64 / known_keys.len() as f64, 4)
    };
    RelationshipSummary {
        total: checked,
        valid,
        broken,
        orphaned,
        coverage,
        issues: ref_issues,
    }
}

/// Public alias so callers outside this module build validation rows.
pub fn rows_from_corpus_items(items: &[CorpusItem]) -> Vec<ValidationRow> {
    rows_from_items(items)
}
