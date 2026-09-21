//! Artifact specs, loaded from the vendored, language-neutral asdecided-spec registry
//! embedded at build time. Field, section, and map order are preserved
//! everywhere (PORT-CONTRACT.d/04 §1, PORT-CONTRACT.d/09, PORT-CONTRACT.d/05 §3.1).
//!
//! The Python registry `ARTIFACT_SPECS` is an ordered tuple of 5 specs:
//! `requirement, decision, roadmap, prompt, design`. That order is
//! load-bearing (classification tie-break, `available_schemas()`, registry
//! iteration). All maps below preserve their JSON insertion order via
//! `Vec<(K, V)>` so lookups and iteration match Python dict semantics.
//!
//! ADR-083 (revised): a corpus may extend the registry with a *spec bundle* —
//! one JSON file in the same shape as the embedded registry, pinned by path
//! and content digest in `.decided/config.yaml`:
//!
//! ```yaml
//! artifact_types:
//!   version: 1
//!   bundle:
//!     path: .decided/artifact-specs.json
//!     digest: sha256:<64 hex>
//! ```
//!
//! The merged registry is built-ins first in embedded order, then admitted
//! bundle elements in file order, so the classification tie-break is stable.
//! A bundle element that collides with a built-in name, duplicates an earlier
//! element, or fails the structural contract is skipped with a warning. A
//! bundle whose bytes do not match the pinned digest — or that cannot be
//! located, read, or parsed — is a hard error, matching the federation pin.
//! With no `artifact_types` stanza nothing here is consulted and every
//! consumer sees exactly the embedded five.

use std::path::{Component, Path, PathBuf};
use std::sync::{Mutex, OnceLock, RwLock};

use serde_json::Value;

use crate::frontmatter::Yaml;

/// Embedded spec data synced from `asdecided/spec`.
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
    /// OKF export `type` for a bundle-declared type (ADR-083 decision 6).
    /// Built-ins leave this unset and use the fixed profile table; a bundle
    /// element without it exports under its `display`.
    pub okf_type: Option<String>,
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
        .map(|o| {
            o.iter()
                .map(|(k, val)| (k.clone(), str_list(val)))
                .collect()
        })
        .unwrap_or_default()
}

/// One parser for both the embedded registry and a bundle element: a bundle
/// element that admits is, field for field, a valid registry element.
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
        okf_type: v["okf_type"]
            .as_str()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string),
    }
}

struct SpecData {
    specs: Vec<ArtifactSpec>,
    relationship_descriptions: Vec<(String, String)>,
}

/// The embedded registry — always the five built-ins, never a bundle.
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

// ---------------------------------------------------------------------------
// Spec bundles (ADR-083 revised): the merged registry and its process slot
// ---------------------------------------------------------------------------

/// Warning code for one skipped bundle element.
pub const CODE_SPEC_SKIPPED: &str = "artifact-spec-skipped";

/// Maximum bundle size in bytes (the ADR-145 bounded-parse posture).
pub const MAX_BUNDLE_BYTES: u64 = 1_048_576;
/// Maximum JSON nodes in a bundle (objects, arrays, and scalars).
pub const MAX_BUNDLE_NODES: usize = 16_384;
/// Maximum declared bundle elements (mirrors the federation parent bound).
pub const MAX_BUNDLE_ELEMENTS: usize = 32;

const DIGEST_PREFIX: &str = "sha256:";

/// The registry element keys, plus the ADR-083 decision 6 extension.
const ELEMENT_KEYS: [&str; 13] = [
    "name",
    "display",
    "required",
    "recommended",
    "optional",
    "metadata",
    "retired_status",
    "descriptions",
    "guidance",
    "synonyms",
    "id_field",
    "starter_bodies",
    "okf_type",
];

/// One skipped bundle element.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BundleWarning {
    /// Always [`CODE_SPEC_SKIPPED`] today; carried so renderers stay generic.
    pub code: &'static str,
    /// The element's declared `name` when it was a usable string.
    pub name: Option<String>,
    /// Zero-based element position in the bundle's `artifact_specs`.
    pub index: usize,
    pub message: String,
}

/// The pin as declared in `.decided/config.yaml`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BundlePin {
    /// POSIX-relative path from the repository root, as declared.
    pub path: String,
    /// `sha256:<64 lowercase hex>` over the file's raw bytes, as declared.
    pub digest: String,
}

/// A verified, parsed, admitted bundle: what `validate` and `doctor` report.
#[derive(Debug, Clone)]
pub struct SpecBundle {
    pub pin: BundlePin,
    /// Admitted type names in bundle order.
    pub admitted: Vec<String>,
    pub warnings: Vec<BundleWarning>,
}

/// Why a pinned bundle could not be loaded. Every variant is a hard error:
/// the corpus declared a bundle and the engine cannot honour the declaration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpecBundleError {
    /// The `artifact_types` stanza is malformed.
    Config(String),
    /// The declared path is not a contained POSIX-relative file path.
    InvalidPath(String),
    /// The declared path does not resolve to a regular file.
    Missing(String),
    /// A symlink or reparse point sits on the declared path.
    Symlink(String),
    /// The file exceeds [`MAX_BUNDLE_BYTES`] or [`MAX_BUNDLE_NODES`].
    TooLarge(String),
    /// The file could not be read.
    Unreadable(String),
    /// The declared digest is not `sha256:` + 64 lowercase hex.
    InvalidDigest(String),
    /// The file's bytes do not hash to the declared digest.
    DigestMismatch { expected: String, actual: String },
    /// The file is not a JSON object with an `artifact_specs` array.
    Parse(String),
}

impl SpecBundleError {
    /// Stable finding code, in the federation family's spelling.
    pub fn stable_code(&self) -> &'static str {
        match self {
            Self::Config(_) => "artifact-spec-bundle-config-invalid",
            Self::InvalidPath(_) => "artifact-spec-bundle-path-invalid",
            Self::Missing(_) => "artifact-spec-bundle-missing",
            Self::Symlink(_) => "artifact-spec-bundle-symlink-traversal",
            Self::TooLarge(_) => "artifact-spec-bundle-limit-exceeded",
            Self::Unreadable(_) => "artifact-spec-bundle-unreadable",
            Self::InvalidDigest(_) => "artifact-spec-bundle-digest-invalid",
            Self::DigestMismatch { .. } => "artifact-spec-bundle-digest-mismatch",
            Self::Parse(_) => "artifact-spec-bundle-parse-failed",
        }
    }
}

impl SpecBundleError {
    /// The human detail without the stable-code prefix (the validation row
    /// carries the code in its own field).
    pub fn detail(&self) -> String {
        match self {
            Self::Config(m)
            | Self::InvalidPath(m)
            | Self::Missing(m)
            | Self::Symlink(m)
            | Self::TooLarge(m)
            | Self::Unreadable(m)
            | Self::InvalidDigest(m)
            | Self::Parse(m) => m.clone(),
            Self::DigestMismatch { expected, actual } => format!(
                "artifact spec bundle bytes hash to {actual}, but .decided/config.yaml pins \
                 {expected}; re-pin the bundle after reviewing the change"
            ),
        }
    }
}

impl std::fmt::Display for SpecBundleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.stable_code(), self.detail())
    }
}

impl std::error::Error for SpecBundleError {}

/// The merged registry: built-ins first, then admitted bundle elements.
#[derive(Debug)]
pub struct Registry {
    specs: Vec<ArtifactSpec>,
    bundle: Option<SpecBundle>,
}

impl Registry {
    /// The embedded five and nothing else.
    pub fn builtin() -> Registry {
        Registry {
            specs: data().specs.clone(),
            bundle: None,
        }
    }

    pub fn specs(&self) -> &[ArtifactSpec] {
        &self.specs
    }

    pub fn spec_for(&self, name: &str) -> Option<&ArtifactSpec> {
        self.specs.iter().find(|s| s.name == name)
    }

    pub fn bundle(&self) -> Option<&SpecBundle> {
        self.bundle.as_ref()
    }
}

/// The process slot. `None` means the embedded registry (no bundle pinned):
/// every reader then touches exactly the same data as before bundles existed.
static ACTIVE: RwLock<Option<&'static Registry>> = RwLock::new(None);

/// Every registry this process has built, keyed by the pin that produced it.
/// A registry is leaked once so `&'static` keeps every consumer signature
/// intact; a later sync that observes a pin already here reuses its registry
/// instead of leaking another copy. The process therefore holds at most one
/// registry per distinct pin it has served, however often a long-running
/// server flips between pins.
static BUILT: Mutex<Vec<(BundlePin, &'static Registry)>> = Mutex::new(Vec::new());

fn active() -> Option<&'static Registry> {
    *ACTIVE
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Point the slot at `registry` for the rest of the process (or until the
/// next [`sync_registry`] observes a different pin).
fn install(registry: Option<&'static Registry>) {
    *ACTIVE
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = registry;
}

/// The registry for `pin`: the one already built for it, else `build` it once
/// and keep it for the life of the process. The pin is the identity because
/// its digest covers the bundle bytes; a bundle edited without a re-pin fails
/// verification in `build` rather than silently replacing a kept registry.
fn registry_for_pin(
    pin: &BundlePin,
    build: impl FnOnce() -> Result<Registry, SpecBundleError>,
) -> Result<&'static Registry, SpecBundleError> {
    {
        let built = BUILT
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some((_, registry)) = built.iter().find(|(known, _)| known == pin) {
            return Ok(registry);
        }
    }
    let registry: &'static Registry = Box::leak(Box::new(build()?));
    let mut built = BUILT
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    // Another thread may have built the same pin meanwhile; keep the first.
    if let Some((_, existing)) = built.iter().find(|(known, _)| known == pin) {
        return Ok(existing);
    }
    built.push((pin.clone(), registry));
    Ok(registry)
}

/// The ordered spec registry (`ARTIFACT_SPECS`): requirement, decision,
/// roadmap, prompt, design — in that exact order — followed by any admitted
/// bundle elements in bundle order.
pub fn specs() -> &'static [ArtifactSpec] {
    match active() {
        Some(registry) => &registry.specs,
        None => &data().specs,
    }
}

/// The embedded built-in specs only, whatever bundle is active.
pub fn builtin_specs() -> &'static [ArtifactSpec] {
    &data().specs
}

/// The spec for a canonical type name, or `None` for `"unknown"` / unregistered.
pub fn spec_for(name: &str) -> Option<&'static ArtifactSpec> {
    specs().iter().find(|s| s.name == name)
}

/// Whether `name` is one of the embedded built-in types.
pub fn is_builtin(name: &str) -> bool {
    data().specs.iter().any(|s| s.name == name)
}

/// `available_schemas()` = the spec names in registry order.
pub fn available_schemas() -> Vec<&'static str> {
    specs().iter().map(|s| s.name.as_str()).collect()
}

/// Canonical relationship-section descriptions, in declared order
/// (`relationship_descriptions` from the JSON; PORT-CONTRACT.d/05).
pub fn relationship_descriptions() -> &'static [(String, String)] {
    &data().relationship_descriptions
}

/// The bundle behind the active registry, when one is pinned.
pub fn active_bundle() -> Option<&'static SpecBundle> {
    active().and_then(|r| r.bundle.as_ref())
}

/// The active bundle's pinned digest, folded into cache generation keys so a
/// re-pin invalidates classification cached under the previous registry.
pub fn active_bundle_digest() -> Option<&'static str> {
    active_bundle().map(|b| b.pin.digest.as_str())
}

/// Plural heading for a bundle-declared type's display name (`Runbooks`,
/// `Policies`): the naive English rule is enough for a heading and keeps the
/// built-in headings (`Requirements`, `Decisions`, …) as they are.
pub fn plural_display(display: &str) -> String {
    let lower = display.to_ascii_lowercase();
    let bytes = lower.as_bytes();
    let ends = |suffix: &str| lower.ends_with(suffix);
    if ends("y") && bytes.len() >= 2 && !b"aeiou".contains(&bytes[bytes.len() - 2]) {
        format!("{}ies", &display[..display.len() - 1])
    } else if ends("s") || ends("x") || ends("z") || ends("ch") || ends("sh") {
        format!("{display}es")
    } else {
        format!("{display}s")
    }
}

/// RAC `type` → OKF `type` (`docs/okf-profile.md`, ADR-048). The five
/// built-in rows are fixed; a bundle type maps to its `okf_type`, defaulting
/// to its `display` (ADR-083 decision 6). `None` for an unregistered type.
pub fn okf_type_for(name: &str) -> Option<String> {
    match name {
        "requirement" => Some("Requirement".to_string()),
        "decision" => Some("ADR".to_string()),
        "design" => Some("Design".to_string()),
        "roadmap" => Some("Roadmap".to_string()),
        "prompt" => Some("Prompt".to_string()),
        other => spec_for(other).map(|spec| {
            spec.okf_type
                .clone()
                .unwrap_or_else(|| spec.display.clone())
        }),
    }
}

// --- config stanza -------------------------------------------------------------

fn yaml_get<'a>(pairs: &'a [(Yaml, Yaml)], key: &str) -> Option<&'a Yaml> {
    pairs.iter().find_map(|(k, v)| match k {
        Yaml::Str(s) if s == key => Some(v),
        _ => None,
    })
}

/// Read the `artifact_types` stanza from a parsed `.decided/config.yaml`.
/// `Ok(None)` when the stanza is absent (the inert case).
pub fn bundle_pin_from_config(config: &Yaml) -> Result<Option<BundlePin>, SpecBundleError> {
    let Yaml::Map(root) = config else {
        return Ok(None);
    };
    let Some(stanza) = yaml_get(root, "artifact_types") else {
        return Ok(None);
    };
    let Yaml::Map(stanza) = stanza else {
        return Err(SpecBundleError::Config(
            "'artifact_types' must be a mapping with 'version' and 'bundle'".to_string(),
        ));
    };
    match yaml_get(stanza, "version") {
        Some(Yaml::Int(1)) => {}
        Some(other) => {
            return Err(SpecBundleError::Config(format!(
                "'artifact_types.version' must be 1, got {}",
                yaml_repr(other)
            )))
        }
        None => {
            return Err(SpecBundleError::Config(
                "'artifact_types' requires 'version: 1'".to_string(),
            ))
        }
    }
    let Some(Yaml::Map(bundle)) = yaml_get(stanza, "bundle") else {
        return Err(SpecBundleError::Config(
            "'artifact_types.bundle' must be a mapping with 'path' and 'digest'".to_string(),
        ));
    };
    for (key, _) in bundle {
        match key {
            Yaml::Str(k) if k == "path" || k == "digest" => {}
            other => {
                return Err(SpecBundleError::Config(format!(
                    "'artifact_types.bundle' has an unknown key {}",
                    yaml_repr(other)
                )))
            }
        }
    }
    let path = match yaml_get(bundle, "path") {
        Some(Yaml::Str(p)) if !p.is_empty() => p.clone(),
        _ => {
            return Err(SpecBundleError::Config(
                "'artifact_types.bundle.path' must be a non-empty string".to_string(),
            ))
        }
    };
    let digest = match yaml_get(bundle, "digest") {
        Some(Yaml::Str(d)) if !d.is_empty() => d.clone(),
        _ => {
            return Err(SpecBundleError::Config(
                "'artifact_types.bundle.digest' must be a non-empty string".to_string(),
            ))
        }
    };
    Ok(Some(BundlePin { path, digest }))
}

fn yaml_repr(value: &Yaml) -> String {
    match value {
        Yaml::Str(s) => format!("'{s}'"),
        Yaml::Int(i) => i.to_string(),
        Yaml::Bool(b) => b.to_string(),
        Yaml::Null => "null".to_string(),
        other => format!("{other:?}"),
    }
}

// --- bundle loading --------------------------------------------------------------

/// Validate the declared path shape and resolve it under `repository_root`,
/// refusing every symlink or reparse point along the way.
fn resolve_bundle_path(repository_root: &Path, declared: &str) -> Result<PathBuf, SpecBundleError> {
    if declared.len() > 4_096 {
        return Err(SpecBundleError::InvalidPath(format!(
            "bundle path exceeds 4096 bytes: {}",
            &declared[..64]
        )));
    }
    if declared.contains('\\') || declared.starts_with('/') || declared.contains(':') {
        return Err(SpecBundleError::InvalidPath(format!(
            "bundle path must be POSIX-relative to the repository root: '{declared}'"
        )));
    }
    for segment in declared.split('/') {
        if segment.is_empty() || segment == "." || segment == ".." {
            return Err(SpecBundleError::InvalidPath(format!(
                "bundle path must not contain empty, '.', or '..' segments: '{declared}'"
            )));
        }
        if segment.len() > 255 {
            return Err(SpecBundleError::InvalidPath(format!(
                "bundle path segment exceeds 255 bytes: '{declared}'"
            )));
        }
    }
    let mut current = repository_root.to_path_buf();
    for segment in declared.split('/') {
        current.push(segment);
        let metadata = std::fs::symlink_metadata(&current).map_err(|error| {
            SpecBundleError::Missing(format!(
                "artifact spec bundle not found at {}: {error}",
                current.display()
            ))
        })?;
        if metadata.file_type().is_symlink() {
            return Err(SpecBundleError::Symlink(format!(
                "artifact spec bundle path crosses a symlink at {}",
                current.display()
            )));
        }
    }
    if current
        .components()
        .any(|c| matches!(c, Component::ParentDir))
    {
        return Err(SpecBundleError::InvalidPath(format!(
            "bundle path escapes the repository root: '{declared}'"
        )));
    }
    let metadata = std::fs::symlink_metadata(&current).map_err(|error| {
        SpecBundleError::Missing(format!(
            "artifact spec bundle not found at {}: {error}",
            current.display()
        ))
    })?;
    if !metadata.is_file() {
        return Err(SpecBundleError::Missing(format!(
            "artifact spec bundle is not a regular file: {}",
            current.display()
        )));
    }
    if metadata.len() > MAX_BUNDLE_BYTES {
        return Err(SpecBundleError::TooLarge(format!(
            "artifact spec bundle exceeds {MAX_BUNDLE_BYTES} bytes: {}",
            current.display()
        )));
    }
    Ok(current)
}

fn valid_digest_literal(digest: &str) -> bool {
    digest.strip_prefix(DIGEST_PREFIX).is_some_and(|hex| {
        hex.len() == 64
            && hex
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    })
}

fn count_nodes(value: &Value, budget: &mut usize) -> bool {
    if *budget == 0 {
        return false;
    }
    *budget -= 1;
    match value {
        Value::Array(items) => items.iter().all(|item| count_nodes(item, budget)),
        Value::Object(map) => map.values().all(|item| count_nodes(item, budget)),
        _ => true,
    }
}

/// Load, verify, parse, and admit the pinned bundle into a merged registry.
/// Hard failures are errors; per-element failures are warnings on the result.
pub fn load_bundle(repository_root: &Path, pin: &BundlePin) -> Result<Registry, SpecBundleError> {
    if !valid_digest_literal(&pin.digest) {
        return Err(SpecBundleError::InvalidDigest(format!(
            "'artifact_types.bundle.digest' must be sha256: followed by 64 lowercase hex \
             characters, got '{}'",
            pin.digest
        )));
    }
    let path = resolve_bundle_path(repository_root, &pin.path)?;
    let bytes = std::fs::read(&path).map_err(|error| {
        SpecBundleError::Unreadable(format!(
            "cannot read artifact spec bundle {}: {error}",
            path.display()
        ))
    })?;
    if bytes.len() as u64 > MAX_BUNDLE_BYTES {
        return Err(SpecBundleError::TooLarge(format!(
            "artifact spec bundle exceeds {MAX_BUNDLE_BYTES} bytes: {}",
            path.display()
        )));
    }
    let actual = format!("{DIGEST_PREFIX}{}", crate::sha256::hexdigest(&bytes));
    if actual != pin.digest {
        return Err(SpecBundleError::DigestMismatch {
            expected: pin.digest.clone(),
            actual,
        });
    }
    let root: Value = serde_json::from_slice(&bytes).map_err(|error| {
        SpecBundleError::Parse(format!(
            "artifact spec bundle {} is not valid JSON: {error}",
            path.display()
        ))
    })?;
    let mut budget = MAX_BUNDLE_NODES;
    if !count_nodes(&root, &mut budget) {
        return Err(SpecBundleError::TooLarge(format!(
            "artifact spec bundle exceeds {MAX_BUNDLE_NODES} JSON nodes: {}",
            path.display()
        )));
    }
    let Some(elements) = root
        .as_object()
        .and_then(|o| o.get("artifact_specs"))
        .and_then(Value::as_array)
    else {
        return Err(SpecBundleError::Parse(format!(
            "artifact spec bundle {} must be a JSON object with an 'artifact_specs' array \
             (the shape of artifact-specs.json)",
            path.display()
        )));
    };
    if elements.len() > MAX_BUNDLE_ELEMENTS {
        return Err(SpecBundleError::TooLarge(format!(
            "artifact spec bundle declares {} elements; at most {MAX_BUNDLE_ELEMENTS} are \
             allowed: {}",
            elements.len(),
            path.display()
        )));
    }

    let mut specs = data().specs.clone();
    let mut admitted = Vec::new();
    let mut warnings = Vec::new();
    for (index, element) in elements.iter().enumerate() {
        let name = element
            .get("name")
            .and_then(Value::as_str)
            .map(str::to_string);
        match admit_element(element, &specs) {
            Ok(spec) => {
                admitted.push(spec.name.clone());
                specs.push(spec);
            }
            Err(reason) => warnings.push(BundleWarning {
                code: CODE_SPEC_SKIPPED,
                name,
                index,
                message: reason,
            }),
        }
    }
    Ok(Registry {
        specs,
        bundle: Some(SpecBundle {
            pin: pin.clone(),
            admitted,
            warnings,
        }),
    })
}

fn valid_type_name(name: &str) -> bool {
    let bytes = name.as_bytes();
    (2..=32).contains(&bytes.len())
        && bytes[0].is_ascii_lowercase()
        && bytes
            .iter()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'_' | b'-'))
}

/// A section name is normalised when it equals its trimmed, casefolded,
/// single-spaced form — the form the section matcher compares against.
fn normalised(section: &str) -> bool {
    let folded = crate::pycompat::py_casefold(section);
    let collapsed: Vec<&str> = folded.split_whitespace().collect();
    section == collapsed.join(" ")
}

fn check_str_list(v: &Value, field: &str, allow_empty: bool) -> Result<(), String> {
    match v {
        Value::Null if allow_empty => Ok(()),
        Value::Array(items) => {
            if items.is_empty() && !allow_empty {
                return Err(format!(
                    "'{field}' must be a non-empty list of section names"
                ));
            }
            for item in items {
                let Some(s) = item.as_str() else {
                    return Err(format!("'{field}' must contain only strings"));
                };
                if s.is_empty() || !normalised(s) {
                    return Err(format!(
                        "'{field}' entry {} is not a normalised section name (trimmed, \
                         lower-case, single-spaced)",
                        crate::pycompat::py_repr_str(s)
                    ));
                }
            }
            Ok(())
        }
        _ => Err(format!("'{field}' must be a list of section names")),
    }
}

fn check_keys_normalised(v: &Value, field: &str) -> Result<(), String> {
    match v {
        Value::Null => Ok(()),
        Value::Object(map) => {
            for key in map.keys() {
                if key.is_empty() || !normalised(key) {
                    return Err(format!(
                        "'{field}' key {} is not a normalised section name",
                        crate::pycompat::py_repr_str(key)
                    ));
                }
            }
            Ok(())
        }
        _ => Err(format!(
            "'{field}' must be a JSON object keyed by section name"
        )),
    }
}

/// The admission guard (ADR-083 decision 2): one element in, a spec out, or
/// the reason it was skipped.
fn admit_element(element: &Value, taken: &[ArtifactSpec]) -> Result<ArtifactSpec, String> {
    let Some(map) = element.as_object() else {
        return Err("element is not a JSON object".to_string());
    };
    if let Some(unknown) = map.keys().find(|k| !ELEMENT_KEYS.contains(&k.as_str())) {
        return Err(format!(
            "unknown key {} (the element shape is that of artifact-specs.json plus okf_type)",
            crate::pycompat::py_repr_str(unknown)
        ));
    }
    let Some(name) = map.get("name").and_then(Value::as_str) else {
        return Err("'name' must be a string".to_string());
    };
    if name == "unknown" || !valid_type_name(name) {
        return Err(format!(
            "name {} is not a valid type name (^[a-z][a-z0-9_-]{{1,31}}$, not 'unknown')",
            crate::pycompat::py_repr_str(name)
        ));
    }
    if is_builtin(name) {
        return Err(format!(
            "name {} collides with a built-in artifact type; built-ins always win",
            crate::pycompat::py_repr_str(name)
        ));
    }
    if taken.iter().any(|s| s.name == name) {
        return Err(format!(
            "name {} duplicates an earlier bundle element; the first declaration wins",
            crate::pycompat::py_repr_str(name)
        ));
    }
    match map.get("display").and_then(Value::as_str) {
        Some(display) if !display.trim().is_empty() => {}
        _ => return Err("'display' must be a non-empty string".to_string()),
    }
    check_str_list(
        map.get("required").unwrap_or(&Value::Null),
        "required",
        false,
    )?;
    check_str_list(
        map.get("recommended").unwrap_or(&Value::Null),
        "recommended",
        true,
    )?;
    check_str_list(
        map.get("optional").unwrap_or(&Value::Null),
        "optional",
        true,
    )?;
    check_str_list(
        map.get("retired_status").unwrap_or(&Value::Null),
        "retired_status",
        true,
    )
    .or_else(|_| {
        // retired_status holds status values, not section names: only the
        // list-of-strings shape is required here.
        match map.get("retired_status") {
            None | Some(Value::Null) => Ok(()),
            Some(Value::Array(items)) if items.iter().all(Value::is_string) => Ok(()),
            _ => Err("'retired_status' must be a list of strings".to_string()),
        }
    })?;
    let metadata = map.get("metadata").unwrap_or(&Value::Null);
    check_keys_normalised(metadata, "metadata")?;
    if let Value::Object(fields) = metadata {
        for (field, values) in fields {
            let Value::Array(items) = values else {
                return Err(format!(
                    "'metadata.{field}' must be a list of allowed values"
                ));
            };
            if items.is_empty()
                || !items
                    .iter()
                    .all(|v| v.as_str().is_some_and(|s| !s.trim().is_empty()))
            {
                return Err(format!(
                    "'metadata.{field}' must be a non-empty list of non-empty strings"
                ));
            }
        }
    }
    if let (Some(Value::Array(retired)), Some(Value::Object(fields))) =
        (map.get("retired_status"), map.get("metadata"))
    {
        if let Some(Value::Array(statuses)) = fields.get("status") {
            if let Some(stray) = retired.iter().find(|r| !statuses.contains(r)) {
                return Err(format!(
                    "'retired_status' entry {} is not in 'metadata.status'",
                    crate::pycompat::py_repr_str(stray.as_str().unwrap_or_default())
                ));
            }
        }
    }
    check_keys_normalised(
        map.get("descriptions").unwrap_or(&Value::Null),
        "descriptions",
    )?;
    check_keys_normalised(map.get("guidance").unwrap_or(&Value::Null), "guidance")?;
    check_keys_normalised(
        map.get("starter_bodies").unwrap_or(&Value::Null),
        "starter_bodies",
    )?;
    match map.get("synonyms") {
        None | Some(Value::Null) => {}
        Some(Value::Object(pairs)) => {
            for (alias, target) in pairs {
                let Some(target) = target.as_str() else {
                    return Err("'synonyms' values must be section names".to_string());
                };
                if alias.is_empty()
                    || !normalised(alias)
                    || target.is_empty()
                    || !normalised(target)
                {
                    return Err(format!(
                        "'synonyms' entry {} -> {} is not normalised",
                        crate::pycompat::py_repr_str(alias),
                        crate::pycompat::py_repr_str(target)
                    ));
                }
            }
        }
        Some(_) => return Err("'synonyms' must be a JSON object".to_string()),
    }
    match map.get("id_field") {
        None | Some(Value::Null) => {}
        Some(Value::String(s)) if !s.is_empty() => {}
        Some(_) => return Err("'id_field' must be null or a non-empty string".to_string()),
    }
    match map.get("okf_type") {
        None | Some(Value::Null) => {}
        Some(Value::String(s)) if !s.trim().is_empty() => {}
        Some(_) => return Err("'okf_type' must be a non-empty string when present".to_string()),
    }
    Ok(build_spec(element))
}

// --- process synchronisation -------------------------------------------------

/// Bring the process registry in line with the corpus at or above `start_dir`.
///
/// No governing config, an unparseable config, or a config without an
/// `artifact_types` stanza installs the embedded registry — the inert case.
/// A stanza installs the pinned bundle's merged registry, reloading only when
/// the pin differs from the one already active. A malformed stanza, a missing
/// or unreadable bundle, or a digest mismatch is a hard error the caller
/// surfaces exactly as it surfaces a federation pin failure.
pub fn sync_registry(start_dir: &str) -> Result<(), SpecBundleError> {
    let Some(config_path) = crate::validate::find_config_file(start_dir) else {
        install(None);
        return Ok(());
    };
    let Ok(text) = std::fs::read_to_string(&config_path) else {
        install(None);
        return Ok(());
    };
    let Ok(config) = crate::frontmatter::yaml_load_config(&text) else {
        // A config the loader cannot read has no readable stanza; every other
        // reader of this file already degrades the same way.
        install(None);
        return Ok(());
    };
    let Some(pin) = bundle_pin_from_config(&config)? else {
        install(None);
        return Ok(());
    };
    if active_bundle().is_some_and(|b| b.pin == pin) {
        return Ok(());
    }
    let repository_root = config_path
        .parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    let registry = registry_for_pin(&pin, || load_bundle(&repository_root, &pin))?;
    install(Some(registry));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(tag: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "asdecided-spec-bundle-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(root.join(".decided")).unwrap();
        root
    }

    fn write_bundle(root: &Path, json: &str) -> BundlePin {
        std::fs::write(root.join(".decided/artifact-specs.json"), json).unwrap();
        BundlePin {
            path: ".decided/artifact-specs.json".to_string(),
            digest: format!("sha256:{}", crate::sha256::hexdigest(json.as_bytes())),
        }
    }

    const RUNBOOK: &str = r#"{"name":"runbook","display":"Runbook","required":["purpose","steps"],"recommended":["rollback"],"optional":["related decisions"],"metadata":{"status":["Draft","Active","Retired"]},"retired_status":["Retired"],"descriptions":{"purpose":"Why"},"guidance":{"purpose":["What?"]},"synonyms":{"procedure":"steps"},"id_field":null,"starter_bodies":{"purpose":"TODO"},"okf_type":"Runbook"}"#;

    #[test]
    fn embedded_registry_is_the_five_builtins_in_order() {
        let names: Vec<&str> = builtin_specs().iter().map(|s| s.name.as_str()).collect();
        assert_eq!(
            names,
            ["requirement", "decision", "roadmap", "prompt", "design"]
        );
        assert!(builtin_specs().iter().all(|s| s.okf_type.is_none()));
        assert_eq!(Registry::builtin().specs().len(), 5);
    }

    #[test]
    fn bundle_appends_after_builtins_in_file_order() {
        let root = scratch("order");
        let policy = RUNBOOK
            .replace("runbook", "policy")
            .replace("Runbook", "Policy");
        let pin = write_bundle(
            &root,
            &format!("{{\"artifact_specs\":[{policy},{RUNBOOK}]}}"),
        );
        let registry = load_bundle(&root, &pin).unwrap();
        let names: Vec<&str> = registry.specs().iter().map(|s| s.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "requirement",
                "decision",
                "roadmap",
                "prompt",
                "design",
                "policy",
                "runbook"
            ]
        );
        let bundle = registry.bundle().unwrap();
        assert_eq!(bundle.admitted, ["policy", "runbook"]);
        assert!(bundle.warnings.is_empty());
        assert_eq!(
            registry.spec_for("runbook").unwrap().okf_type.as_deref(),
            Some("Runbook")
        );
        assert_eq!(
            registry.spec_for("runbook").unwrap().synonym("procedure"),
            Some("steps")
        );
    }

    #[test]
    fn builtin_collision_and_duplicate_warn_and_skip() {
        let root = scratch("collide");
        let decision = RUNBOOK.replace("\"runbook\"", "\"decision\"");
        let pin = write_bundle(
            &root,
            &format!("{{\"artifact_specs\":[{decision},{RUNBOOK},{RUNBOOK}]}}"),
        );
        let registry = load_bundle(&root, &pin).unwrap();
        let bundle = registry.bundle().unwrap();
        assert_eq!(bundle.admitted, ["runbook"]);
        assert_eq!(bundle.warnings.len(), 2);
        assert_eq!(bundle.warnings[0].index, 0);
        assert!(bundle.warnings[0].message.contains("built-ins always win"));
        assert_eq!(bundle.warnings[1].index, 2);
        assert!(bundle.warnings[1]
            .message
            .contains("first declaration wins"));
        // The built-in decision spec is untouched.
        assert_eq!(
            registry.spec_for("decision").unwrap().required,
            builtin_specs()[1].required
        );
    }

    #[test]
    fn structural_defects_warn_and_skip_with_reasons() {
        let root = scratch("defects");
        let cases = [
            (
                r#"{"name":"Run Book","display":"x","required":["a"]}"#,
                "valid type name",
            ),
            (
                r#"{"name":"unknown","display":"x","required":["a"]}"#,
                "valid type name",
            ),
            (
                r#"{"name":"nodisplay","display":"","required":["a"]}"#,
                "'display'",
            ),
            (
                r#"{"name":"noreq","display":"x","required":[]}"#,
                "'required'",
            ),
            (
                r#"{"name":"caps","display":"x","required":["Purpose"]}"#,
                "normalised",
            ),
            (
                r#"{"name":"extra","display":"x","required":["a"],"loader":"x"}"#,
                "unknown key",
            ),
            (
                r#"{"name":"emptyokf","display":"x","required":["a"],"okf_type":" "}"#,
                "'okf_type'",
            ),
            (
                r#"{"name":"retired","display":"x","required":["a"],"metadata":{"status":["A"]},"retired_status":["B"]}"#,
                "'retired_status'",
            ),
            (r#"[]"#, "not a JSON object"),
        ];
        let elements: Vec<&str> = cases.iter().map(|(json, _)| *json).collect();
        let pin = write_bundle(
            &root,
            &format!("{{\"artifact_specs\":[{}]}}", elements.join(",")),
        );
        let registry = load_bundle(&root, &pin).unwrap();
        let bundle = registry.bundle().unwrap();
        assert!(bundle.admitted.is_empty());
        assert_eq!(bundle.warnings.len(), cases.len());
        for (warning, (_, needle)) in bundle.warnings.iter().zip(cases.iter()) {
            assert!(
                warning.message.contains(needle),
                "{}: {}",
                warning.index,
                warning.message
            );
            assert_eq!(warning.code, CODE_SPEC_SKIPPED);
        }
        assert_eq!(registry.specs().len(), 5);
    }

    #[test]
    fn digest_mismatch_is_a_hard_error() {
        let root = scratch("digest");
        let mut pin = write_bundle(&root, &format!("{{\"artifact_specs\":[{RUNBOOK}]}}"));
        let good = pin.digest.clone();
        pin.digest = format!("sha256:{}", "0".repeat(64));
        let error = load_bundle(&root, &pin).unwrap_err();
        assert_eq!(error.stable_code(), "artifact-spec-bundle-digest-mismatch");
        assert!(error.to_string().contains(&good));
        pin.digest = "md5:abc".to_string();
        assert_eq!(
            load_bundle(&root, &pin).unwrap_err().stable_code(),
            "artifact-spec-bundle-digest-invalid"
        );
    }

    #[test]
    fn missing_escaping_and_malformed_bundles_are_hard_errors() {
        let root = scratch("hard");
        let pin = BundlePin {
            path: ".decided/absent.json".to_string(),
            digest: format!("sha256:{}", "a".repeat(64)),
        };
        assert_eq!(
            load_bundle(&root, &pin).unwrap_err().stable_code(),
            "artifact-spec-bundle-missing"
        );
        for bad in [
            "../x.json",
            "/abs.json",
            ".decided//x.json",
            ".decided/./x.json",
            "a\\b",
        ] {
            let pin = BundlePin {
                path: bad.to_string(),
                digest: format!("sha256:{}", "a".repeat(64)),
            };
            assert_eq!(
                load_bundle(&root, &pin).unwrap_err().stable_code(),
                "artifact-spec-bundle-path-invalid",
                "{bad}"
            );
        }
        let pin = write_bundle(&root, "{\"artifact_specs\": 3}");
        assert_eq!(
            load_bundle(&root, &pin).unwrap_err().stable_code(),
            "artifact-spec-bundle-parse-failed"
        );
        let pin = write_bundle(&root, "not json");
        assert_eq!(
            load_bundle(&root, &pin).unwrap_err().stable_code(),
            "artifact-spec-bundle-parse-failed"
        );
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_bundle_is_refused() {
        let root = scratch("symlink");
        std::fs::write(root.join("real.json"), "{\"artifact_specs\":[]}").unwrap();
        std::os::unix::fs::symlink(root.join("real.json"), root.join(".decided/link.json"))
            .unwrap();
        let pin = BundlePin {
            path: ".decided/link.json".to_string(),
            digest: format!(
                "sha256:{}",
                crate::sha256::hexdigest(b"{\"artifact_specs\":[]}")
            ),
        };
        assert_eq!(
            load_bundle(&root, &pin).unwrap_err().stable_code(),
            "artifact-spec-bundle-symlink-traversal"
        );
    }

    #[test]
    fn config_stanza_parsing() {
        let parse = |text: &str| {
            bundle_pin_from_config(&crate::frontmatter::yaml_load_config(text).unwrap())
        };
        assert_eq!(parse("repository_key: RAC\n").unwrap(), None);
        let ok = parse(
            "artifact_types:\n  version: 1\n  bundle:\n    path: .decided/b.json\n    digest: sha256:ab\n",
        )
        .unwrap()
        .unwrap();
        assert_eq!(ok.path, ".decided/b.json");
        assert_eq!(ok.digest, "sha256:ab");
        for bad in [
            "artifact_types: 1\n",
            "artifact_types:\n  bundle:\n    path: x\n    digest: y\n",
            "artifact_types:\n  version: 2\n  bundle:\n    path: x\n    digest: y\n",
            "artifact_types:\n  version: 1\n",
            "artifact_types:\n  version: 1\n  bundle:\n    path: x\n",
            "artifact_types:\n  version: 1\n  bundle:\n    path: x\n    digest: y\n    extra: z\n",
        ] {
            assert_eq!(
                parse(bad).unwrap_err().stable_code(),
                "artifact-spec-bundle-config-invalid",
                "{bad}"
            );
        }
    }

    #[test]
    fn a_pin_is_built_and_leaked_at_most_once() {
        let root = scratch("memo");
        let pin = write_bundle(&root, &format!("{{\"artifact_specs\":[{RUNBOOK}]}}"));
        let mut builds = 0;
        let first = registry_for_pin(&pin, || {
            builds += 1;
            load_bundle(&root, &pin)
        })
        .unwrap();
        let second = registry_for_pin(&pin, || {
            builds += 1;
            load_bundle(&root, &pin)
        })
        .unwrap();
        assert!(std::ptr::eq(first, second));
        assert_eq!(builds, 1);
        assert_eq!(first.spec_for("runbook").unwrap().display, "Runbook");
        // A different pin is its own registry; a failing build keeps nothing.
        let mut other = pin.clone();
        other.digest = format!("sha256:{}", "1".repeat(64));
        assert!(registry_for_pin(&other, || load_bundle(&root, &other)).is_err());
        let third = registry_for_pin(&pin, || unreachable!("memoised")).unwrap();
        assert!(std::ptr::eq(first, third));
    }

    #[test]
    fn plural_display_headings() {
        assert_eq!(plural_display("Runbook"), "Runbooks");
        assert_eq!(plural_display("Policy"), "Policies");
        assert_eq!(plural_display("Process"), "Processes");
        assert_eq!(plural_display("Day"), "Days");
    }

    #[test]
    fn okf_type_mapping_defaults_to_display() {
        assert_eq!(okf_type_for("decision").as_deref(), Some("ADR"));
        assert_eq!(okf_type_for("requirement").as_deref(), Some("Requirement"));
        assert_eq!(okf_type_for("nope"), None);
        let root = scratch("okf");
        let policy = RUNBOOK
            .replace(",\"okf_type\":\"Runbook\"", "")
            .replace("runbook", "policy")
            .replace("\"Runbook\"", "\"Policy\"");
        let pin = write_bundle(
            &root,
            &format!("{{\"artifact_specs\":[{RUNBOOK},{policy}]}}"),
        );
        let registry = load_bundle(&root, &pin).unwrap();
        assert_eq!(registry.spec_for("policy").unwrap().okf_type, None);
        assert_eq!(registry.spec_for("policy").unwrap().display, "Policy");
        assert_eq!(
            registry.spec_for("runbook").unwrap().okf_type.as_deref(),
            Some("Runbook")
        );
    }
}
