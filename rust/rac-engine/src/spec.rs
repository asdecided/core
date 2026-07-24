//! Artifact specs, loaded from the vendored, language-neutral asdecided-spec registry
//! embedded at build time. Field, section, and map order are preserved
//! everywhere (PORT-CONTRACT.d/04 §1, PORT-CONTRACT.d/09, PORT-CONTRACT.d/05 §3.1).
//!
//! The Python registry `ARTIFACT_SPECS` is an ordered tuple of 5 specs:
//! `requirement, decision, roadmap, prompt, design`. That order is
//! load-bearing (classification tie-break, `available_schemas()`, registry
//! iteration). All maps below preserve their JSON insertion order via
//! `Vec<(K, V)>` so lookups and iteration match Python dict semantics.

use std::sync::OnceLock;

use serde_json::Value;

/// Embedded spec data synced from `itsthelore/asdecided-spec`.
const SPEC_JSON: &str = include_str!("../assets/spec/artifact-specs.json");

/// One artifact type's schema. Field names/order mirror the Python dataclass.
#[derive(Debug, Clone)]
pub struct ArtifactSpec {
    /// Canonical key, e.g. `"requirement"`.
    pub name: String,
    /// Human label, e.g. `"Requirement"`.
    pub display: String,
    /// Sections that define the type (scored at 1.0).
    pub required: Vec<String>,
    /// Expected-but-optional sections (scored at 0.5).
    pub recommended: Vec<String>,
    /// Recognized/extracted sections, never scored, never "missing".
    pub optional: Vec<String>,
    /// `{section -> allowed values}`, in declared order.
    pub metadata: Vec<(String, Vec<String>)>,
    /// Subset of `metadata["status"]` marking retirement.
    pub retired_status: Vec<String>,
    /// Schema-render description hints (`{section -> text}`), declared order.
    pub descriptions: Vec<(String, String)>,
    /// Improve/template guidance hints (`{section -> [lines]}`), declared order.
    pub guidance: Vec<(String, Vec<String>)>,
    /// Alt heading -> canonical section, applied before matching (per-spec).
    pub synonyms: Vec<(String, String)>,
    /// Canonical-id section; no spec sets it today (always `None`).
    pub id_field: Option<String>,
    /// Template starter bodies (`{section -> text}`), declared order.
    pub starter_bodies: Vec<(String, String)>,
}

impl ArtifactSpec {
    /// `expected` (Python property) = `required + recommended`, in that order.
    pub fn expected(&self) -> Vec<String> {
        let mut out = Vec::with_capacity(self.required.len() + self.recommended.len());
        out.extend(self.required.iter().cloned());
        out.extend(self.recommended.iter().cloned());
        out
    }

    /// Allowed values for a metadata field, preserving declared order.
    pub fn metadata_values(&self, field: &str) -> Option<&[String]> {
        self.metadata
            .iter()
            .find(|(k, _)| k == field)
            .map(|(_, v)| v.as_slice())
    }

    /// Canonical section a synonym maps to, if this spec declares one.
    pub fn synonym(&self, heading: &str) -> Option<&str> {
        self.synonyms
            .iter()
            .find(|(k, _)| k == heading)
            .map(|(_, v)| v.as_str())
    }
}

/// The canonical relationship-section vocabulary (`references.py`,
/// PORT-CONTRACT.d/05 §3.1). Order is load-bearing: it is the canonical
/// aggregation order for stats/relationship counts. Each entry is
/// `(canonical space name, snake key)`.
///
/// ```text
/// RELATED_SECTIONS  = related requirements, related decisions,
///                     related roadmaps, related prompts, related designs
/// EXTERNAL_SECTIONS = related tickets, verified by
/// SCOPE_SECTIONS    = applies to
/// RELATIONSHIP_SECTIONS = RELATED_SECTIONS + (supersedes,) + EXTERNAL + SCOPE
/// ```
pub const RELATIONSHIP_SECTIONS: [(&str, &str); 9] = [
    ("related requirements", "related_requirements"),
    ("related decisions", "related_decisions"),
    ("related roadmaps", "related_roadmaps"),
    ("related prompts", "related_prompts"),
    ("related designs", "related_designs"),
    ("supersedes", "supersedes"),
    ("related tickets", "related_tickets"),
    ("verified by", "verified_by"),
    ("applies to", "applies_to"),
];

/// `_snake(section)` = `section.replace(" ", "_")` (spaces -> underscores only).
pub fn snake(section: &str) -> String {
    section.replace(' ', "_")
}

/// `canonical_value` tail: match `candidate` against the allowed vocabulary by
/// casefold equality — the canonical allowed spelling wins, otherwise the
/// candidate passes through. Callers supply their own first-line extraction.
pub fn canonical_value(candidate: &str, allowed: &[String]) -> String {
    let folded = crate::pycompat::py_casefold(candidate);
    for value in allowed {
        if crate::pycompat::py_casefold(value) == folded {
            return value.clone();
        }
    }
    candidate.to_string()
}

// --- JSON extraction helpers -------------------------------------------------

fn as_str(v: &Value) -> String {
    v.as_str().unwrap_or_default().to_string()
}

fn str_list(v: &Value) -> Vec<String> {
    v.as_array()
        .map(|a| a.iter().map(as_str).collect())
        .unwrap_or_default()
}

/// Ordered `{key -> string}` map from a JSON object (insertion order preserved
/// because serde_json is built with the `preserve_order` feature).
fn str_map(v: &Value) -> Vec<(String, String)> {
    v.as_object()
        .map(|o| o.iter().map(|(k, val)| (k.clone(), as_str(val))).collect())
        .unwrap_or_default()
}

/// Ordered `{key -> [string]}` map from a JSON object.
fn list_map(v: &Value) -> Vec<(String, Vec<String>)> {
    v.as_object()
        .map(|o| o.iter().map(|(k, val)| (k.clone(), str_list(val))).collect())
        .unwrap_or_default()
}

fn build_spec(v: &Value) -> ArtifactSpec {
    ArtifactSpec {
        name: as_str(&v["name"]),
        display: as_str(&v["display"]),
        required: str_list(&v["required"]),
        recommended: str_list(&v["recommended"]),
        optional: str_list(&v["optional"]),
        metadata: list_map(&v["metadata"]),
        retired_status: str_list(&v["retired_status"]),
        descriptions: str_map(&v["descriptions"]),
        guidance: list_map(&v["guidance"]),
        synonyms: str_map(&v["synonyms"]),
        id_field: v["id_field"].as_str().map(str::to_string),
        starter_bodies: str_map(&v["starter_bodies"]),
    }
}

struct SpecData {
    specs: Vec<ArtifactSpec>,
    relationship_descriptions: Vec<(String, String)>,
}

fn data() -> &'static SpecData {
    static DATA: OnceLock<SpecData> = OnceLock::new();
    DATA.get_or_init(|| {
        let root: Value = serde_json::from_str(SPEC_JSON).expect("artifact-specs.json parses");
        let specs = root["artifact_specs"]
            .as_array()
            .expect("artifact_specs is an array")
            .iter()
            .map(build_spec)
            .collect();
        let relationship_descriptions = str_map(&root["relationship_descriptions"]);
        SpecData {
            specs,
            relationship_descriptions,
        }
    })
}

/// The ordered spec registry (`ARTIFACT_SPECS`): requirement, decision,
/// roadmap, prompt, design — in that exact order.
pub fn specs() -> &'static [ArtifactSpec] {
    &data().specs
}

/// The spec for a canonical type name, or `None` for `"unknown"` / unregistered.
pub fn spec_for(name: &str) -> Option<&'static ArtifactSpec> {
    data().specs.iter().find(|s| s.name == name)
}

/// `available_schemas()` = the spec names in registry order.
pub fn available_schemas() -> Vec<&'static str> {
    data().specs.iter().map(|s| s.name.as_str()).collect()
}

/// Canonical relationship-section descriptions, in declared order
/// (`relationship_descriptions` from the JSON; PORT-CONTRACT.d/05).
pub fn relationship_descriptions() -> &'static [(String, String)] {
    &data().relationship_descriptions
}
