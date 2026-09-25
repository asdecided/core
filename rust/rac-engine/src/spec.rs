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
//!
//! ADR-150: in a federated closure the effective registry also carries every
//! parent's admitted bundle types, composed bottom-up by `spec_composition`;
//! the stanza's optional `overrides` list adjudicates a name two sources
//! declare differently.

use std::path::{Component, Path, PathBuf};
use std::sync::{Mutex, OnceLock, RwLock};

use serde_json::Value;

use crate::frontmatter::Yaml;

/// Embedded spec data synced from `asdecided/spec`.
const SPEC_JSON: &str = include_str!("../assets/spec/artifact-specs.json");

/// One artifact type's schema. Field names/order mirror the Python dataclass.
#[derive(Debug, Clone, PartialEq, Eq)]
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
///                     related roadmaps, related prompts, related designs,
///                     related risks
/// EXTERNAL_SECTIONS = related tickets, verified by
/// SCOPE_SECTIONS    = applies to
/// RELATIONSHIP_SECTIONS = RELATED_SECTIONS + (supersedes,) + EXTERNAL + SCOPE
/// ```
pub const RELATIONSHIP_SECTIONS: [(&str, &str); 10] = [
    ("related requirements", "related_requirements"),
    ("related decisions", "related_decisions"),
    ("related roadmaps", "related_roadmaps"),
    ("related prompts", "related_prompts"),
    ("related designs", "related_designs"),
    ("related risks", "related_risks"),
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

/// Two sources declare one type name with different content and the
/// composing corpus records no override (ADR-150 decision 2).
pub const CODE_TYPE_CONFLICT: &str = "corpus-federation-artifact-type-conflict";

/// An `artifact_types.overrides` entry the engine cannot honour (ADR-150
/// decision 3): the same finding the artifact-override family uses.
pub const CODE_INVALID_TYPE_OVERRIDE: &str = crate::graph_composition::FINDING_INVALID_OVERRIDE;

/// Type names a bundle may not declare because they are keys the engine
/// already emits beside per-type families (`stats --json` top-level keys) or
/// the reserved `unknown`; a bundle type with one of these names would
/// overwrite an existing field.
const RESERVED_TYPE_NAMES: &[&str] = &[
    "unknown",
    "directory",
    "empty",
    "features",
    "valid_features",
    "invalid_features",
    "requirements",
    "metrics",
    "risks",
    "features_missing_metrics",
    "features_missing_risks",
    "missing_metrics",
    "missing_risks",
    "average_requirements_per_feature",
    "largest_feature",
    "requirements_by_feature",
    "invalid",
    "decisions",
    "roadmaps",
    "prompts",
    "designs",
    "unrecognized",
    "relationships",
    "risk_artifacts",
];

/// The OKF types the built-ins export under (the fixed profile table).
/// A bundle type may not export under one of them: an OKF consumer could not
/// tell it from the built-in (ADR-083 decision 6, ADR-122, ADR-151).
const BUILTIN_OKF_TYPES: &[&str] = &["Requirement", "ADR", "Design", "Roadmap", "Prompt", "Risk"];

/// A label that is safe to render in headings and frontmatter: one line, no
/// control characters, no surrounding whitespace, non-empty.
fn single_line_label(value: &str) -> bool {
    !value.is_empty() && value.trim() == value && !value.chars().any(char::is_control)
}

/// The literal `prefer` value naming the declaring corpus itself.
pub const OVERRIDE_PREFER_LOCAL: &str = "local";

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
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpecBundle {
    pub pin: BundlePin,
    /// Admitted type names in bundle order.
    pub admitted: Vec<String>,
    pub warnings: Vec<BundleWarning>,
}

/// One entry of `artifact_types.overrides` (ADR-150 decision 3): which
/// source's element wins a named collision, backed by a local Decision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeOverride {
    pub name: String,
    /// [`OVERRIDE_PREFER_LOCAL`] or a global corpus source in the declaring
    /// corpus's inherited view.
    pub prefer: String,
    /// Canonical identifier of a live local Decision of the declaring corpus.
    pub rationale: String,
}

/// The parsed `artifact_types` stanza: a pin, overrides, or both.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SpecStanza {
    pub pin: Option<BundlePin>,
    pub overrides: Vec<TypeOverride>,
}

/// One source's contribution to the effective registry (ADR-150 decision 6).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceSpecBundle {
    /// The contributing corpus's `corpus.source`; `None` only for a local
    /// bundle in a config that declares no source (serialised as `null`, never
    /// as the `prefer: local` keyword).
    pub source: Option<String>,
    pub layer: crate::corpus::Layer,
    pub bundle: SpecBundle,
}

/// An override applied while composing the effective registry, kept so its
/// rationale can be checked once the closure's artifacts are parsed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppliedOverride {
    pub owner: String,
    pub name: String,
    pub prefer: String,
    pub rationale: String,
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
    /// Two sources declare `name` with different content and `owner`, the
    /// corpus composing them, records no override for it.
    TypeConflict {
        owner: String,
        name: String,
        sources: Vec<String>,
    },
    /// An `artifact_types.overrides` entry of `owner` that cannot be honoured.
    InvalidOverride {
        owner: String,
        name: String,
        reason: String,
    },
    /// A bundle failure in an inherited source, reported with that source.
    InSource {
        source: String,
        error: Box<SpecBundleError>,
    },
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
            Self::TypeConflict { .. } => CODE_TYPE_CONFLICT,
            Self::InvalidOverride { .. } => CODE_INVALID_TYPE_OVERRIDE,
            Self::InSource { error, .. } => error.stable_code(),
        }
    }

    /// The source the failure belongs to, when it is not the local corpus.
    pub fn source(&self) -> Option<&str> {
        match self {
            Self::InSource { source, .. } => Some(source),
            Self::TypeConflict { owner, .. } | Self::InvalidOverride { owner, .. } => Some(owner),
            _ => None,
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
            Self::TypeConflict {
                owner,
                name,
                sources,
            } => format!(
                "artifact type '{name}' is declared with different content by {}; '{owner}' \
                 must record a Decision and select one under artifact_types.overrides \
                 (ADR-150)",
                sources
                    .iter()
                    .map(|s| format!("'{s}'"))
                    .collect::<Vec<_>>()
                    .join(" and ")
            ),
            Self::InvalidOverride {
                owner,
                name,
                reason,
            } => format!("artifact_types.overrides entry '{name}' in '{owner}': {reason}"),
            Self::InSource { source, error } => {
                format!("inherited source '{source}': {}", error.detail())
            }
        }
    }
}

impl std::fmt::Display for SpecBundleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.stable_code(), self.detail())
    }
}

impl std::error::Error for SpecBundleError {}

/// The merged registry: built-ins first, then admitted bundle elements — the
/// local corpus's own, then (ADR-150) every inherited element in composition
/// order.
#[derive(Debug)]
pub struct Registry {
    specs: Vec<ArtifactSpec>,
    /// The local corpus's own bundle, when it pins one (the ADR-083 report).
    bundle: Option<SpecBundle>,
    /// Every source that pinned a bundle, in composition order.
    sources: Vec<SourceSpecBundle>,
    /// Overrides applied while composing, in composition order.
    overrides: Vec<AppliedOverride>,
    /// Built from a verified federation rather than the local config alone.
    federated: bool,
    /// SHA-256 of the local governing config bytes this registry was built
    /// under; `sync_registry` keeps the registry while they are unchanged.
    local_config_digest: String,
}

impl Registry {
    /// The embedded five and nothing else.
    pub fn builtin() -> Registry {
        Registry {
            specs: data().specs.clone(),
            bundle: None,
            sources: Vec::new(),
            overrides: Vec::new(),
            federated: false,
            local_config_digest: String::new(),
        }
    }

    /// Assemble a registry from its parts (the composer's constructor).
    pub(crate) fn from_parts(
        specs: Vec<ArtifactSpec>,
        bundle: Option<SpecBundle>,
        sources: Vec<SourceSpecBundle>,
        overrides: Vec<AppliedOverride>,
        federated: bool,
        local_config_digest: String,
    ) -> Registry {
        Registry {
            specs,
            bundle,
            sources,
            overrides,
            federated,
            local_config_digest,
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

    pub fn sources(&self) -> &[SourceSpecBundle] {
        &self.sources
    }

    pub fn overrides(&self) -> &[AppliedOverride] {
        &self.overrides
    }

    pub fn is_federated(&self) -> bool {
        self.federated
    }
}

/// The process slot. `None` means the embedded registry (no bundle pinned):
/// every reader then touches exactly the same data as before bundles existed.
static ACTIVE: RwLock<Option<&'static Registry>> = RwLock::new(None);

/// Every registry this process has built, keyed by the governing config bytes
/// that produced it (every source's, in composition order — the pins and the
/// overrides are inside them). A registry is leaked once so `&'static` keeps
/// every consumer signature intact; a later sync that observes a key already
/// here reuses its registry instead of leaking another copy. The process
/// therefore holds at most one registry per distinct key it has served,
/// however often a long-running server flips between pins.
static BUILT: Mutex<Vec<(String, &'static Registry)>> = Mutex::new(Vec::new());

const REGISTRY_KEY_DOMAIN: &[u8] = b"asdecided-spec-registry-key-v1\0";

/// The memo key for a registry: a digest over `(source, config bytes)` frames
/// in composition order. Bundle bytes are not framed; they are re-verified
/// against their pin before the memo is consulted.
pub(crate) fn registry_key(frames: &[(&str, &[u8])]) -> String {
    let mut hasher = crate::sha256::Sha256::new();
    hasher.update(REGISTRY_KEY_DOMAIN);
    for (source, config) in frames {
        hasher.update(&(source.len() as u64).to_be_bytes());
        hasher.update(source.as_bytes());
        hasher.update(&(config.len() as u64).to_be_bytes());
        hasher.update(config);
    }
    hasher.hexdigest()
}

fn active() -> Option<&'static Registry> {
    *ACTIVE
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Point the slot at `registry` for the rest of the process (or until the
/// next [`sync_registry`] or composition observes a different key).
pub(crate) fn install(registry: Option<&'static Registry>) {
    *ACTIVE
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = registry;
}

/// The registry for `key`: the one already built for it, else `build` it once
/// and keep it for the life of the process. Callers verify every bundle's
/// bytes against its pin before asking, so a bundle edited without a re-pin
/// fails closed rather than reaching a kept registry.
pub(crate) fn registry_for_key(
    key: &str,
    build: impl FnOnce() -> Result<Registry, SpecBundleError>,
) -> Result<&'static Registry, SpecBundleError> {
    {
        let built = BUILT
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some((_, registry)) = built.iter().find(|(known, _)| known == key) {
            return Ok(registry);
        }
    }
    let registry: &'static Registry = Box::leak(Box::new(build()?));
    let mut built = BUILT
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    // Another thread may have built the same key meanwhile; keep the first.
    if let Some((_, existing)) = built.iter().find(|(known, _)| known == key) {
        return Ok(existing);
    }
    built.push((key.to_string(), registry));
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

/// The built-in families whose validation, stats, and OKF surfaces are hand
/// coded and pinned by released JSON shapes. Every other registered type —
/// a later built-in such as `risk` (ADR-151 decision 7) or a bundle type —
/// takes the registry-driven generic path.
const HAND_CODED_FAMILIES: [&str; 5] = ["requirement", "decision", "roadmap", "prompt", "design"];

/// Whether `name` is one of the hand-coded families (see
/// [`HAND_CODED_FAMILIES`]); `false` for a registry-driven type.
pub fn is_hand_coded(name: &str) -> bool {
    HAND_CODED_FAMILIES.contains(&name)
}

/// The `stats --json` family key for a registry-driven type: the type name,
/// except where an accepted decision fixes another to avoid confusion with an
/// existing field (ADR-151 decision 6: `risks` already counts Requirement
/// risk lines, so the Risk family is `risk_artifacts`).
pub fn stats_family_key(name: &str) -> &str {
    match name {
        "risk" => "risk_artifacts",
        other => other,
    }
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

/// The active registry, when one other than the embedded five is installed.
pub fn active_registry() -> Option<&'static Registry> {
    active()
}

/// A stable identity for the installed registry: its address, which never
/// changes because registries are leaked once and memoised; 0 for the
/// embedded built-ins. Long-lived servers compare it across requests.
pub fn active_registry_identity() -> usize {
    active().map_or(0, |registry| registry as *const Registry as usize)
}

/// The local corpus's bundle behind the active registry, when it pins one.
pub fn active_bundle() -> Option<&'static SpecBundle> {
    active().and_then(|r| r.bundle.as_ref())
}

/// Every source contributing a bundle to the active registry, in composition
/// order; empty when nothing is pinned anywhere in the closure.
pub fn active_sources() -> &'static [SourceSpecBundle] {
    active().map_or(&[], |r| r.sources.as_slice())
}

/// The local bundle's pinned digest.
pub fn active_bundle_digest() -> Option<&'static str> {
    active_bundle().map(|b| b.pin.digest.as_str())
}

/// Every effective bundle digest in composition order (ADR-150 decision 5),
/// folded into cache generation keys so a re-pin anywhere in the closure
/// invalidates classification cached under the previous registry. With one
/// local bundle this is exactly the ADR-083 single digest.
pub fn active_bundle_digests() -> Vec<&'static str> {
    active_sources()
        .iter()
        .map(|s| s.bundle.pin.digest.as_str())
        .collect()
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

/// RAC `type` → OKF `type` (`docs/okf-profile.md`, ADR-048). The built-in
/// rows are fixed (`risk` → `Risk`, ADR-151 decision 6); a bundle type maps to its `okf_type`, defaulting
/// to its `display` (ADR-083 decision 6). `None` for an unregistered type.
pub fn okf_type_for(name: &str) -> Option<String> {
    match name {
        "requirement" => Some("Requirement".to_string()),
        "decision" => Some("ADR".to_string()),
        "design" => Some("Design".to_string()),
        "roadmap" => Some("Roadmap".to_string()),
        "prompt" => Some("Prompt".to_string()),
        "risk" => Some("Risk".to_string()),
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

/// Read the bundle pin from a parsed `.decided/config.yaml`. `Ok(None)` when
/// the stanza is absent or declares only overrides.
pub fn bundle_pin_from_config(config: &Yaml) -> Result<Option<BundlePin>, SpecBundleError> {
    Ok(spec_stanza_from_config(config)?.and_then(|stanza| stanza.pin))
}

/// Read the `artifact_types` stanza from a parsed `.decided/config.yaml`.
/// `Ok(None)` when the stanza is absent (the inert case).
pub fn spec_stanza_from_config(config: &Yaml) -> Result<Option<SpecStanza>, SpecBundleError> {
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
    for (key, _) in stanza {
        match key {
            Yaml::Str(k) if k == "version" || k == "bundle" || k == "overrides" => {}
            other => {
                return Err(SpecBundleError::Config(format!(
                    "'artifact_types' has an unknown key {}",
                    yaml_repr(other)
                )))
            }
        }
    }
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
    let overrides = match yaml_get(stanza, "overrides") {
        None => Vec::new(),
        Some(value) => type_overrides_from_yaml(value)?,
    };
    let bundle = match yaml_get(stanza, "bundle") {
        Some(Yaml::Map(bundle)) => bundle,
        None if !overrides.is_empty() => {
            return Ok(Some(SpecStanza {
                pin: None,
                overrides,
            }))
        }
        _ => {
            return Err(SpecBundleError::Config(
                "'artifact_types.bundle' must be a mapping with 'path' and 'digest'".to_string(),
            ));
        }
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
    Ok(Some(SpecStanza {
        pin: Some(BundlePin { path, digest }),
        overrides,
    }))
}

/// The `overrides` list (ADR-150 decision 3): each entry exactly `name`,
/// `prefer`, and `rationale`, all non-empty strings; `prefer` is `local` or a
/// well-formed global corpus source; a name appears at most once.
fn type_overrides_from_yaml(value: &Yaml) -> Result<Vec<TypeOverride>, SpecBundleError> {
    let Yaml::List(items) = value else {
        return Err(SpecBundleError::Config(
            "'artifact_types.overrides' must be a non-empty list of mappings".to_string(),
        ));
    };
    if items.is_empty() {
        return Err(SpecBundleError::Config(
            "'artifact_types.overrides' must be a non-empty list of mappings".to_string(),
        ));
    }
    let mut overrides: Vec<TypeOverride> = Vec::with_capacity(items.len());
    for (index, item) in items.iter().enumerate() {
        let Yaml::Map(fields) = item else {
            return Err(SpecBundleError::Config(format!(
                "'artifact_types.overrides[{index}]' must be a mapping with 'name', 'prefer', \
                 and 'rationale'"
            )));
        };
        for (key, _) in fields {
            match key {
                Yaml::Str(k) if k == "name" || k == "prefer" || k == "rationale" => {}
                other => {
                    return Err(SpecBundleError::Config(format!(
                        "'artifact_types.overrides[{index}]' has an unknown key {}",
                        yaml_repr(other)
                    )))
                }
            }
        }
        let field = |name: &str| -> Result<String, SpecBundleError> {
            match yaml_get(fields, name) {
                Some(Yaml::Str(s)) if !s.trim().is_empty() => Ok(s.clone()),
                _ => Err(SpecBundleError::Config(format!(
                    "'artifact_types.overrides[{index}].{name}' must be a non-empty string"
                ))),
            }
        };
        let name = field("name")?;
        if !valid_type_name(&name) {
            return Err(SpecBundleError::Config(format!(
                "'artifact_types.overrides[{index}].name' {} is not a valid type name",
                crate::pycompat::py_repr_str(&name)
            )));
        }
        let prefer = field("prefer")?;
        if prefer != OVERRIDE_PREFER_LOCAL && !crate::scaffold::valid_corpus_source(&prefer) {
            return Err(SpecBundleError::Config(format!(
                "'artifact_types.overrides[{index}].prefer' must be 'local' or a global corpus \
                 source, got {}",
                crate::pycompat::py_repr_str(&prefer)
            )));
        }
        let rationale = field("rationale")?;
        if overrides.iter().any(|o| o.name == name) {
            return Err(SpecBundleError::Config(format!(
                "'artifact_types.overrides' names {} more than once; a name may be overridden \
                 at most once per corpus",
                crate::pycompat::py_repr_str(&name)
            )));
        }
        overrides.push(TypeOverride {
            name,
            prefer,
            rationale,
        });
    }
    Ok(overrides)
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
            declared.chars().take(64).collect::<String>()
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
    let (path, bytes) = read_verified_bundle(repository_root, pin)?;
    admit_bundle(&path, &bytes, pin)
}

/// Locate the pinned bundle under `repository_root`, read it within bounds,
/// and verify its bytes against the pin. Nothing is parsed here.
pub fn read_verified_bundle(
    repository_root: &Path,
    pin: &BundlePin,
) -> Result<(PathBuf, Vec<u8>), SpecBundleError> {
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
    Ok((path, bytes))
}

/// Parse verified bundle bytes and admit their elements after the built-ins.
pub fn admit_bundle(
    path: &Path,
    bytes: &[u8],
    pin: &BundlePin,
) -> Result<Registry, SpecBundleError> {
    let root: Value = serde_json::from_slice(bytes).map_err(|error| {
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
        sources: Vec::new(),
        overrides: Vec::new(),
        federated: false,
        local_config_digest: String::new(),
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
    if RESERVED_TYPE_NAMES.contains(&name) {
        return Err(format!(
            "name {} is reserved: it is a field the engine already emits beside the \
             per-type families",
            crate::pycompat::py_repr_str(name)
        ));
    }
    if !valid_type_name(name) {
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
        Some(display) if single_line_label(display) => {}
        _ => {
            return Err(
                "'display' must be a non-empty single-line string without surrounding \
                 whitespace"
                    .to_string(),
            )
        }
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
    // Values, not only keys: an ill-typed value would otherwise be admitted
    // and silently turned into an empty string or list.
    for field in ["descriptions", "starter_bodies"] {
        if let Some(Value::Object(entries)) = map.get(field) {
            if let Some((key, _)) = entries.iter().find(|(_, v)| !v.is_string()) {
                return Err(format!("'{field}.{key}' must be a string"));
            }
        }
    }
    if let Some(Value::Object(entries)) = map.get("guidance") {
        if let Some((key, _)) = entries.iter().find(|(_, v)| {
            !v.as_array()
                .is_some_and(|items| items.iter().all(Value::is_string))
        }) {
            return Err(format!("'guidance.{key}' must be a list of strings"));
        }
    }
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
        Some(Value::String(s)) if single_line_label(s) => {
            if BUILTIN_OKF_TYPES
                .iter()
                .any(|builtin| builtin.eq_ignore_ascii_case(s))
            {
                return Err(format!(
                    "'okf_type' {} is a built-in OKF type; a bundle type must export under \
                     its own",
                    crate::pycompat::py_repr_str(s)
                ));
            }
        }
        Some(_) => {
            return Err(
                "'okf_type' must be a non-empty single-line string without surrounding \
                 whitespace when present"
                    .to_string(),
            )
        }
    }
    Ok(build_spec(element))
}

// --- process synchronisation -------------------------------------------------

/// Parse governing config bytes. `Ok(None)` for a config the loader cannot
/// read that says nothing about artifact types: it has no stanza, and every
/// other reader of the file degrades the same way. A config that mentions
/// `artifact_types` but cannot be parsed is a hard error, because its pin can
/// be neither honoured nor ignored (ADR-083 decision 2).
pub(crate) fn parse_config_bytes(bytes: &[u8]) -> Result<Option<Yaml>, SpecBundleError> {
    let mentions_stanza = bytes
        .windows(b"artifact_types".len())
        .any(|window| window == b"artifact_types");
    let unreadable = |problem: String| {
        if mentions_stanza {
            Err(SpecBundleError::Config(format!(
                ".decided/config.yaml declares artifact_types but cannot be parsed: {problem}"
            )))
        } else {
            Ok(None)
        }
    };
    let Ok(text) = std::str::from_utf8(bytes) else {
        return unreadable("it is not valid UTF-8".to_string());
    };
    match crate::frontmatter::yaml_load_config(text) {
        Ok(config) => Ok(Some(config)),
        Err(problem) => unreadable(problem),
    }
}

/// The local corpus source named by a governing config, when it declares one.
fn local_source(config_path: &Path, text: &str) -> Option<String> {
    crate::scaffold::parse_identity_config(&config_path.display().to_string(), text)
        .ok()
        .and_then(|identity| identity.corpus_source)
}

/// Bring the process registry in line with the corpus at or above `start_dir`.
///
/// No governing config, an unparseable config, or a config without an
/// `artifact_types` stanza installs the embedded registry — the inert case.
/// A stanza installs the pinned bundle's merged registry, rebuilding only when
/// the governing config bytes differ from the ones the active registry was
/// built under; a registry composed from a verified federation (ADR-150) is
/// kept on the same terms while the manifest is still present. The local
/// bundle's bytes are re-verified against the pin on every sync. A malformed
/// stanza, a missing or unreadable bundle, or a digest mismatch is a hard
/// error the caller surfaces exactly as it surfaces a federation pin failure.
pub fn sync_registry(start_dir: &str) -> Result<(), SpecBundleError> {
    let Some(config_path) = crate::validate::find_config_file(start_dir) else {
        install(None);
        return Ok(());
    };
    let Ok(bytes) = std::fs::read(&config_path) else {
        install(None);
        return Ok(());
    };
    let repository_root = config_path
        .parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    let manifest_present =
        std::fs::symlink_metadata(repository_root.join(crate::federation::MANIFEST_RELATIVE_PATH))
            .is_ok();
    let config_digest = crate::sha256::hexdigest(&bytes);
    if let Some(current) = active() {
        if current.local_config_digest == config_digest && (!current.federated || manifest_present)
        {
            if let Some(bundle) = &current.bundle {
                read_verified_bundle(&repository_root, &bundle.pin)?;
            }
            return Ok(());
        }
    }
    let config = match parse_config_bytes(&bytes) {
        Ok(Some(config)) => config,
        Ok(None) => {
            install(None);
            return Ok(());
        }
        Err(error) => return Err(error),
    };
    let text = std::str::from_utf8(&bytes).unwrap_or_default();
    let Some(stanza) = spec_stanza_from_config(&config)? else {
        install(None);
        return Ok(());
    };
    let declared_source = local_source(&config_path, text);
    let source = declared_source
        .clone()
        .unwrap_or_else(|| OVERRIDE_PREFER_LOCAL.into());
    if !manifest_present {
        if let Some(first) = stanza.overrides.first() {
            // Nothing can collide in a corpus that declares no parents.
            return Err(SpecBundleError::InvalidOverride {
                owner: source,
                name: first.name.clone(),
                reason: "the corpus declares no parents, so no artifact type can collide"
                    .to_string(),
            });
        }
    }
    let Some(pin) = stanza.pin else {
        // Overrides without a bundle: nothing local to admit. Composition of
        // the federated closure applies them (ADR-150).
        install(None);
        return Ok(());
    };
    let (path, bundle_bytes) = read_verified_bundle(&repository_root, &pin)?;
    let key = registry_key(&[(source.as_str(), bytes.as_slice())]);
    let registry = registry_for_key(&key, || {
        let mut registry = admit_bundle(&path, &bundle_bytes, &pin)?;
        registry.sources = vec![SourceSpecBundle {
            source: declared_source.clone(),
            layer: crate::corpus::Layer::Local,
            bundle: registry
                .bundle
                .clone()
                .expect("an admitted bundle carries its report"),
        }];
        registry.local_config_digest = config_digest.clone();
        Ok(registry)
    })?;
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
    fn embedded_registry_is_the_six_builtins_in_order() {
        let names: Vec<&str> = builtin_specs().iter().map(|s| s.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "requirement",
                "decision",
                "roadmap",
                "prompt",
                "design",
                "risk"
            ]
        );
        assert!(builtin_specs().iter().all(|s| s.okf_type.is_none()));
        assert_eq!(Registry::builtin().specs().len(), 6);
    }

    #[test]
    fn risk_is_a_registry_driven_builtin_not_a_hand_coded_family() {
        // Built-in precedence (classification, bundle names) sees six; the
        // hand-coded output paths see five, so Risk takes the generic path
        // (ADR-151 decision 7).
        assert!(is_builtin("risk"));
        assert!(!is_hand_coded("risk"));
        for name in ["requirement", "decision", "roadmap", "prompt", "design"] {
            assert!(is_builtin(name) && is_hand_coded(name), "{name}");
        }
        assert_eq!(stats_family_key("risk"), "risk_artifacts");
        assert_eq!(stats_family_key("runbook"), "runbook");
        assert_eq!(okf_type_for("risk").as_deref(), Some("Risk"));
        let risk = spec_for("risk").unwrap();
        assert_eq!(risk.required, ["risk", "likelihood", "impact"]);
        assert_eq!(risk.recommended, ["context", "assumptions"]);
        assert_eq!(risk.optional[0], "mitigation");
        assert!(!risk.optional.iter().any(|s| s == "related prompts"));
        for declarer in ["requirement", "decision", "roadmap", "prompt", "design"] {
            assert!(
                spec_for(declarer)
                    .unwrap()
                    .optional
                    .iter()
                    .any(|s| s == "related risks"),
                "{declarer} declares related risks"
            );
        }
        assert!(RELATIONSHIP_SECTIONS.contains(&("related risks", "related_risks")));
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
                "risk",
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
                "reserved",
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
            (
                r#"{"name":"decisions","display":"x","required":["a"]}"#,
                "reserved",
            ),
            (
                r#"{"name":"twolines","display":"A\nB","required":["a"]}"#,
                "'display'",
            ),
            (
                r#"{"name":"injected","display":"x","required":["a"],"okf_type":"Runbook\nid: FORGED"}"#,
                "'okf_type'",
            ),
            (
                r#"{"name":"spoof","display":"x","required":["a"],"okf_type":"adr"}"#,
                "built-in OKF type",
            ),
            (
                r#"{"name":"baddesc","display":"x","required":["a"],"descriptions":{"a":42}}"#,
                "'descriptions.a'",
            ),
            (
                r#"{"name":"badbody","display":"x","required":["a"],"starter_bodies":{"a":{"x":1}}}"#,
                "'starter_bodies.a'",
            ),
            (
                r#"{"name":"badguide","display":"x","required":["a"],"guidance":{"a":"Ask why"}}"#,
                "'guidance.a'",
            ),
            (
                r#"{"name":"risk","display":"x","required":["a"]}"#,
                "built-ins always win",
            ),
            (
                r#"{"name":"risk_artifacts","display":"x","required":["a"]}"#,
                "reserved",
            ),
            (
                r#"{"name":"hazard","display":"x","required":["a"],"okf_type":"Risk"}"#,
                "built-in OKF type",
            ),
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
        assert_eq!(registry.specs().len(), 6);
    }

    #[test]
    fn a_long_multibyte_bundle_path_is_an_error_not_a_panic() {
        let root = scratch("multibyte");
        let declared = format!("{}{}", "a/".repeat(21), "€".repeat(1400));
        let pin = BundlePin {
            path: declared,
            digest: format!("sha256:{}", "0".repeat(64)),
        };
        let error = load_bundle(&root, &pin).unwrap_err();
        assert_eq!(error.stable_code(), "artifact-spec-bundle-path-invalid");
    }

    #[test]
    fn an_unparseable_config_that_mentions_the_stanza_is_a_hard_error() {
        let error = parse_config_bytes(b"artifact_types: [unclosed\n").unwrap_err();
        assert_eq!(error.stable_code(), "artifact-spec-bundle-config-invalid");
        let error = parse_config_bytes(b"artifact_types:\xff\n").unwrap_err();
        assert_eq!(error.stable_code(), "artifact-spec-bundle-config-invalid");
        // Without the stanza an unreadable config stays inert, as released.
        assert!(parse_config_bytes(b"extra: [unclosed\n").unwrap().is_none());
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
    fn a_key_is_built_and_leaked_at_most_once() {
        let root = scratch("memo");
        let pin = write_bundle(&root, &format!("{{\"artifact_specs\":[{RUNBOOK}]}}"));
        let key = registry_key(&[("acme/memo", pin.digest.as_bytes())]);
        let mut builds = 0;
        let first = registry_for_key(&key, || {
            builds += 1;
            load_bundle(&root, &pin)
        })
        .unwrap();
        let second = registry_for_key(&key, || {
            builds += 1;
            load_bundle(&root, &pin)
        })
        .unwrap();
        assert!(std::ptr::eq(first, second));
        assert_eq!(builds, 1);
        assert_eq!(first.spec_for("runbook").unwrap().display, "Runbook");
        // A different key is its own registry; a failing build keeps nothing.
        let mut other = pin.clone();
        other.digest = format!("sha256:{}", "1".repeat(64));
        let other_key = registry_key(&[("acme/memo", other.digest.as_bytes())]);
        assert!(registry_for_key(&other_key, || load_bundle(&root, &other)).is_err());
        let third = registry_for_key(&key, || unreachable!("memoised")).unwrap();
        assert!(std::ptr::eq(first, third));
        // The key frames source and bytes in order; neither is interchangeable.
        assert_ne!(key, registry_key(&[("acme/other", pin.digest.as_bytes())]));
        assert_ne!(
            registry_key(&[("a", b"x"), ("b", b"y")]),
            registry_key(&[("b", b"y"), ("a", b"x")])
        );
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
