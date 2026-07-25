//! Scaffold writes — `decided new`, `decided init`, `decided quickstart`,
//! `decided migrate metadata` (PORT-CONTRACT.d/16).
//!
//! Ports of `src/rac/core/idgen.py` (`generate_id`), `src/rac/core/
//! templates.py` (`load_template`), `src/asdecided/services/init.py`
//! (`init_repository`, `load_repository_config`, `write_mcp_configs` via
//! `src/asdecided/services/profiles.py`), `src/asdecided/services/create.py`
//! (`create_artifact`), `src/asdecided/services/quickstart.py` (`quickstart`),
//! and `src/asdecided/services/migrate.py` (`migrate_metadata`).
//!
//! The packaged template bodies are embedded verbatim from
//! `rust/decided-engine/assets/templates/`, vendored byte-identical copies of
//! the Python package files — a unit test below pins that identity, because
//! the written artifact must be byte-identical to what the oracle writes.
//!
//! Minted ids are wall-clock + CSPRNG derived (the oracle has no external
//! seam); the parity harness masks them (`mask-ids`) on stdout AND captured
//! file bytes, so this module uses the real clock and `/dev/urandom`.

use std::collections::HashSet;
use std::path::Path;

use crate::pycompat::py_repr_str;
use crate::relationships::corpus_items;
use crate::spec::available_schemas;
use crate::validate::find_config_file;
use crate::walk::py_join;

// ---------------------------------------------------------------------------
// Errors (decided.services.{create,init,quickstart,migrate} exception classes)
// ---------------------------------------------------------------------------

/// The scaffold failure contract, message-shaped like the oracle's
/// exception `str()`. Exit-code routing lives with each command handler,
/// because the SAME error class maps to different exits per command
/// (`OutputPathExists` is usage exit 2 under `new` but a refusal exit 1
/// under `quickstart` — measured).
pub enum ScaffoldError {
    /// `TemplateNotFound` — unsupported artifact type (usage).
    TemplateNotFound(String),
    /// `OutputPathExists` — never overwrite (new: exit 2; quickstart: 1).
    OutputPathExists(String),
    /// `OutputDirectoryMissing` — no auto-create (usage).
    OutputDirectoryMissing(String),
    /// `MissingRepositoryConfig` — run `decided init` first (usage).
    MissingRepositoryConfig(String),
    /// `InvalidRepositoryKey` — bad key syntax (usage).
    InvalidRepositoryKey(String),
    /// `RepositoryKeyConflict` — established key differs (exit 1).
    RepositoryKeyConflict(String),
    /// `MalformedRepositoryConfig` — unreadable config (exit 1).
    MalformedRepositoryConfig(String),
    /// `IdGenerationExhausted` — broken entropy source (exit 1).
    IdGenerationExhausted(String),
    /// `CorpusNotEmpty` — quickstart refuses a non-empty corpus (exit 1).
    CorpusNotEmpty(String),
    /// `InvalidOrgEndpoint` — non-http(s) org endpoint URL (usage).
    InvalidOrgEndpoint(String),
    /// `MalformedClientConfig` — unmergeable MCP client config (exit 1).
    MalformedClientConfig(String),
}

impl ScaffoldError {
    pub fn message(&self) -> &str {
        match self {
            ScaffoldError::TemplateNotFound(m)
            | ScaffoldError::OutputPathExists(m)
            | ScaffoldError::OutputDirectoryMissing(m)
            | ScaffoldError::MissingRepositoryConfig(m)
            | ScaffoldError::InvalidRepositoryKey(m)
            | ScaffoldError::RepositoryKeyConflict(m)
            | ScaffoldError::MalformedRepositoryConfig(m)
            | ScaffoldError::IdGenerationExhausted(m)
            | ScaffoldError::CorpusNotEmpty(m)
            | ScaffoldError::InvalidOrgEndpoint(m)
            | ScaffoldError::MalformedClientConfig(m) => m,
        }
    }
}

fn template_not_found(artifact_type: &str) -> ScaffoldError {
    ScaffoldError::TemplateNotFound(format!(
        "unsupported artifact type: {artifact_type} (supported: {})",
        available_schemas().join(", ")
    ))
}

fn missing_repository_config(start_dir: &str) -> ScaffoldError {
    ScaffoldError::MissingRepositoryConfig(format!(
        "no repository identity found at or above {start_dir}; \
         run `decided init` to establish a repository key first"
    ))
}

fn malformed_config(config_path: &str, reason: &str) -> ScaffoldError {
    ScaffoldError::MalformedRepositoryConfig(format!(
        "malformed repository config {config_path}: {reason}"
    ))
}

fn id_generation_exhausted() -> ScaffoldError {
    ScaffoldError::IdGenerationExhausted(format!(
        "could not generate a unique artifact ID in {MAX_ID_ATTEMPTS} attempts"
    ))
}

// ---------------------------------------------------------------------------
// Opaque id generation (decided.core.idgen, ADR-026)
// ---------------------------------------------------------------------------

/// Crockford base32: no I, L, O, U (visually ambiguous).
pub const ID_ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

const TIME_CHARS: usize = 8; // 40 bits of millisecond timestamp
const RANDOM_CHARS: usize = 4; // 20 bits of CSPRNG entropy

/// Bounded regeneration attempts on an index collision (create/migrate).
const MAX_ID_ATTEMPTS: usize = 5;

fn encode_base32(mut value: u64, chars: usize) -> String {
    let mut out = vec![0u8; chars];
    for slot in out.iter_mut().rev() {
        *slot = ID_ALPHABET[(value & 0x1F) as usize];
        value >>= 5;
    }
    String::from_utf8(out).expect("alphabet is ASCII")
}

/// 20 bits of CSPRNG entropy (`secrets.randbits(20)`), from /dev/urandom
/// with a time/pid fallback so id minting never fails outright.
fn random_bits_20() -> u64 {
    use std::io::Read;
    let mut buf = [0u8; 4];
    if std::fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut buf))
        .is_ok()
    {
        return (u64::from(u32::from_le_bytes(buf))) & 0xF_FFFF;
    }
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos() as u64)
        .unwrap_or(0);
    (nanos ^ u64::from(std::process::id())) & 0xF_FFFF
}

/// `generate_id(repository_key)` — `<KEY>-` + 8-char millisecond-timestamp
/// segment + 4-char random segment, Crockford base32.
pub fn generate_id(repository_key: &str) -> String {
    let millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
        & ((1 << (TIME_CHARS * 5)) - 1);
    format!(
        "{repository_key}-{}{}",
        encode_base32(millis, TIME_CHARS),
        encode_base32(random_bits_20(), RANDOM_CHARS)
    )
}

// ---------------------------------------------------------------------------
// Canonical templates (decided.core.templates, ADR-021)
// ---------------------------------------------------------------------------

/// The embedded template bodies, index-aligned with `available_schemas()`
/// registry order (requirement, decision, roadmap, prompt, design).
const TEMPLATE_BYTES: [&str; 5] = [
    include_str!("../assets/templates/requirement.md"),
    include_str!("../assets/templates/decision.md"),
    include_str!("../assets/templates/roadmap.md"),
    include_str!("../assets/templates/prompt.md"),
    include_str!("../assets/templates/design.md"),
];

/// `load_template(artifact_type)` — the canonical body, or
/// `TemplateNotFound` for an unregistered type. `TemplateResourceMissing`
/// (a broken Python installation) has no Rust equivalent: embedded
/// resources cannot be absent from a linked binary.
pub fn load_template(artifact_type: &str) -> Result<&'static str, ScaffoldError> {
    available_schemas()
        .iter()
        .position(|name| *name == artifact_type)
        .map(|i| TEMPLATE_BYTES[i])
        .ok_or_else(|| template_not_found(artifact_type))
}

/// `render_frontmatter(artifact_id, artifact_type)` — canonical generated
/// envelope, stable key order (v0.7.11 contract).
pub fn render_frontmatter(artifact_id: &str, artifact_type: &str) -> String {
    format!("---\nschema_version: 1\nid: {artifact_id}\ntype: {artifact_type}\n---\n")
}

// ---------------------------------------------------------------------------
// Repository identity config (decided.services.init)
// ---------------------------------------------------------------------------

/// `KEY_RE = ^[A-Z][A-Z0-9]{1,9}$` — with Python `$` also matching just
/// before one trailing newline (`re.match` semantics).
fn valid_repository_key(key: &str) -> bool {
    let core = key.strip_suffix('\n').unwrap_or(key);
    let b = core.as_bytes();
    (2..=10).contains(&b.len())
        && b[0].is_ascii_uppercase()
        && b.iter().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
}

fn invalid_key_error(key: &str) -> ScaffoldError {
    ScaffoldError::InvalidRepositoryKey(format!(
        "invalid repository key: {} (expected 2-10 uppercase \
         alphanumeric characters starting with a letter, e.g. RAC)",
        py_repr_str(key)
    ))
}

/// A discovered repository identity configuration.
pub struct RepositoryConfig {
    pub repository_key: String,
    pub config_path: String,
}

/// `_read_config(config_path)` — strict read of one config file: YAML must
/// parse (the invalid-YAML reason embeds this engine's own problem text —
/// the oracle embeds PyYAML's; stderr-only divergence class), the root must
/// be a mapping with a string `repository_key` matching the key contract.
fn read_config(config_path: &str) -> Result<RepositoryConfig, ScaffoldError> {
    let text = std::fs::read_to_string(config_path)
        .map_err(|e| malformed_config(config_path, &format!("invalid YAML: {e}")))?;
    let data = crate::frontmatter::yaml_load_config(&text)
        .map_err(|problem| malformed_config(config_path, &format!("invalid YAML: {problem}")))?;
    let key = match &data {
        crate::frontmatter::Yaml::Map(pairs) => pairs.iter().find_map(|(k, v)| match (k, v) {
            (crate::frontmatter::Yaml::Str(name), crate::frontmatter::Yaml::Str(value))
                if name == "repository_key" =>
            {
                Some(value.clone())
            }
            _ => None,
        }),
        _ => None,
    };
    let Some(key) = key else {
        return Err(malformed_config(
            config_path,
            "missing required string field 'repository_key'",
        ));
    };
    if !valid_repository_key(&key) {
        return Err(malformed_config(
            config_path,
            &format!("invalid repository_key: {}", py_repr_str(&key)),
        ));
    }
    Ok(RepositoryConfig {
        repository_key: key,
        config_path: config_path.to_string(),
    })
}

/// `load_repository_config(start_dir)` — the nearest `.decided/config.yaml` at
/// or above the RESOLVED `start_dir`, read strictly, or None.
pub fn load_repository_config(start_dir: &str) -> Result<Option<RepositoryConfig>, ScaffoldError> {
    match find_config_file(start_dir) {
        Some(path) => read_config(&path.to_string_lossy()).map(Some),
        None => Ok(None),
    }
}

// ---------------------------------------------------------------------------
// Init profiles (decided.services.profiles, ADR-088)
// ---------------------------------------------------------------------------

/// The lore MCP server wiring, identical for Claude Code (`.mcp.json`) and
/// Cursor (`.cursor/mcp.json`).
pub const MCP_JSON: &str = "{\n  \"mcpServers\": {\n    \"asdecided\": {\n      \"command\": \"decided-mcp\",\n      \"args\": [\"--root\", \".\"]\n    }\n  }\n}\n";

/// The enterprise profile's committed enforcement stanza (ADR-049/088) —
/// appended verbatim after the repository key.
const ENTERPRISE_CONFIG: &str = "\
# Enterprise profile (ADR-088): relationship-integrity findings block `decided gate`,
# committed explicitly so the enforcement policy is auditable (ADR-049).
enforcement:
  blocking:
    - relationship-target-not-found
    - relationship-target-ambiguous
    - relationship-self-reference
    - relationship-target-type-mismatch
    - relationship-target-superseded
    - relationship-cycle
    - relationship-edge-unsupported
    - duplicate-artifact-identifier
";

/// `(config_stanza, mcp_wiring)` for a built-in profile name. The CLI's
/// argparse choices already reject unknown names (`InvalidProfile` is
/// unreachable from the CLI, like the oracle).
fn profile_parts(profile: &str) -> (&'static str, bool) {
    match profile {
        "enterprise" => (ENTERPRISE_CONFIG, true),
        _ => ("", true), // "default"
    }
}

/// `write_mcp_configs(directory)` — write the client wiring, never
/// overwriting; returns the paths actually written, target order.
fn write_mcp_configs(directory: &str) -> std::io::Result<Vec<String>> {
    let targets: [&[&str]; 2] = [&[".mcp.json"], &[".cursor", "mcp.json"]];
    let mut written = Vec::new();
    for target in targets {
        let path = py_join(directory, target);
        if Path::new(&path).exists() {
            continue;
        }
        if let Some(parent) = Path::new(&path).parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, MCP_JSON)?;
        written.push(path);
    }
    Ok(written)
}

/// The shared org endpoint's server name in client configs (ADR-117).
const ORG_SERVER_KEY: &str = "lore-org";

fn invalid_org_endpoint(url: &str) -> ScaffoldError {
    ScaffoldError::InvalidOrgEndpoint(format!(
        "invalid org endpoint: {} (expected an http:// or https:// URL, \
         e.g. https://lore.example.com/mcp)",
        py_repr_str(url)
    ))
}

fn malformed_client_config(config_path: &str, reason: &str) -> ScaffoldError {
    ScaffoldError::MalformedClientConfig(format!(
        "malformed MCP client config {config_path}: {reason}"
    ))
}

/// The `lore-org` streamable-HTTP server entry for `url` (ADR-117),
/// insertion-ordered like the oracle's dict literal.
fn org_server_entry(url: &str) -> serde_json::Value {
    let mut entry = serde_json::Map::new();
    entry.insert("type".to_string(), serde_json::Value::String("http".to_string()));
    entry.insert("url".to_string(), serde_json::Value::String(url.to_string()));
    serde_json::Value::Object(entry)
}

/// `write_org_endpoint(directory, url)` — ensure the `lore-org` entry in
/// each client config (profiles.write_org_endpoint, ADR-117): merge into an
/// existing file touching only the `lore-org` key, create absent files,
/// skip files already carrying the exact entry, and parse every target
/// before writing any (no partial writes). `serde_json`'s `preserve_order`
/// keeps user key order exactly as the oracle's `dict` does.
fn write_org_endpoint(directory: &str, url: &str) -> Result<Vec<String>, ScaffoldError> {
    let entry = org_server_entry(url);
    let targets: [&[&str]; 2] = [&[".mcp.json"], &[".cursor", "mcp.json"]];
    let mut planned: Vec<(String, String)> = Vec::new();
    for target in targets {
        let path = py_join(directory, target);
        if Path::new(&path).is_file() {
            let text = std::fs::read_to_string(&path)
                .map_err(|_| malformed_client_config(&path, "not valid JSON"))?;
            let mut data: serde_json::Value = serde_json::from_str(&text)
                .map_err(|_| malformed_client_config(&path, "not valid JSON"))?;
            let obj = data.as_object_mut().ok_or_else(|| {
                malformed_client_config(&path, "top level must be a JSON object")
            })?;
            let servers = obj
                .entry("mcpServers")
                .or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()));
            let servers = servers.as_object_mut().ok_or_else(|| {
                malformed_client_config(&path, "'mcpServers' must be a JSON object")
            })?;
            if servers.get(ORG_SERVER_KEY) == Some(&entry) {
                continue; // already wired to this endpoint: idempotent no-op
            }
            servers.insert(ORG_SERVER_KEY.to_string(), entry.clone());
            planned.push((path, format!("{}\n", crate::pyjson::dumps_indent2_no_ascii(&data))));
        } else {
            let mut servers = serde_json::Map::new();
            servers.insert(ORG_SERVER_KEY.to_string(), entry.clone());
            let mut payload = serde_json::Map::new();
            payload.insert("mcpServers".to_string(), serde_json::Value::Object(servers));
            let payload = serde_json::Value::Object(payload);
            planned.push((path, format!("{}\n", crate::pyjson::dumps_indent2_no_ascii(&payload))));
        }
    }
    let mut written = Vec::new();
    for (path, text) in planned {
        if let Some(parent) = Path::new(&path).parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| malformed_client_config(&path, &e.to_string()))?;
        }
        std::fs::write(&path, text).map_err(|e| malformed_client_config(&path, &e.to_string()))?;
        written.push(path);
    }
    Ok(written)
}

/// Outcome of one `decided init` run (stable JSON contract, ADR-007).
pub struct InitResult {
    pub repository_key: String,
    pub config_path: String,
    pub created: bool,
    pub profile: Option<String>,
    pub files_written: Vec<String>,
    pub org_endpoint: Option<String>,
}

/// `init_repository(directory, key, ticketing, profile, org_endpoint)` —
/// establish (or confirm) the identity namespace. `ticketing` and `profile`
/// arrive argparse-choice-validated; both apply only on a FRESH init.
/// `org_endpoint` (ADR-117) is an explicit operator action and applies on
/// fresh AND already-initialized repositories alike.
pub fn init_repository(
    directory: &str,
    key: &str,
    ticketing: Option<&str>,
    profile: Option<&str>,
    org_endpoint: Option<&str>,
) -> Result<InitResult, ScaffoldError> {
    if !valid_repository_key(key) {
        return Err(invalid_key_error(key));
    }
    if let Some(url) = org_endpoint {
        if !(url.starts_with("http://") || url.starts_with("https://")) {
            return Err(invalid_org_endpoint(url));
        }
    }
    let config_path = py_join(directory, &[".decided", "config.yaml"]);
    if Path::new(&config_path).is_file() {
        let existing = read_config(&config_path)?;
        if existing.repository_key != key {
            return Err(ScaffoldError::RepositoryKeyConflict(format!(
                "repository already initialized with key {} ({config_path}); \
                 refusing to change it to {} \u{2014} established ID namespaces \
                 are never silently rewritten",
                py_repr_str(&existing.repository_key),
                py_repr_str(key)
            )));
        }
        let org_files = match org_endpoint {
            Some(url) => write_org_endpoint(directory, url)?,
            None => Vec::new(),
        };
        return Ok(InitResult {
            repository_key: key.to_string(),
            config_path,
            created: false,
            profile: None,
            files_written: org_files,
            org_endpoint: org_endpoint.map(str::to_string),
        });
    }
    let io_err = |e: std::io::Error| malformed_config(&config_path, &format!("invalid YAML: {e}"));
    if let Some(parent) = Path::new(&config_path).parent() {
        std::fs::create_dir_all(parent).map_err(io_err)?;
    }
    let mut body = format!("repository_key: {key}\n");
    if let Some(provider) = ticketing {
        body.push_str(&format!("ticketing:\n  provider: {provider}\n"));
    }
    let (stanza, wiring) = profile.map(profile_parts).unwrap_or(("", false));
    body.push_str(stanza);
    std::fs::write(&config_path, body).map_err(io_err)?;
    let mut files_written = if profile.is_some() && wiring {
        write_mcp_configs(directory).map_err(io_err)?
    } else {
        Vec::new()
    };
    if let Some(url) = org_endpoint {
        for path in write_org_endpoint(directory, url)? {
            if !files_written.contains(&path) {
                files_written.push(path);
            }
        }
    }
    Ok(InitResult {
        repository_key: key.to_string(),
        config_path,
        created: true,
        profile: profile.map(str::to_string),
        files_written,
        org_endpoint: org_endpoint.map(str::to_string),
    })
}

// ---------------------------------------------------------------------------
// Artifact creation (decided.services.create)
// ---------------------------------------------------------------------------

/// Result of one artifact creation (`bytes_written` is in the oracle's
/// dataclass but deliberately absent from its JSON, so it is not carried).
pub struct CreatedArtifact {
    pub artifact_type: String,
    pub path: String,
    pub id: String,
}

/// `str(Path(p))` / `str(Path(p).parent)` — the pathlib shaping used by
/// the error messages (the SUCCESS path echoes the argv string verbatim).
fn py_parent(p: &str) -> String {
    let normalized = crate::walk::normalize_root(p);
    if normalized == "/" || normalized == "." {
        return normalized;
    }
    match normalized.rfind('/') {
        Some(0) => "/".to_string(),
        Some(i) => normalized[..i].to_string(),
        None => ".".to_string(),
    }
}

/// The id-collision set: every discovered artifact's canonical identifier,
/// uppercased (`{entry.id.upper() for entry in build_repository_index(...)}`).
///
/// The oracle CRASHES here when the walk hits hostile markdown (an
/// unhashable YAML key raises inside frontmatter parsing — the pinned
/// oracle-crash class); the native walk is total, so hostile files simply
/// contribute whatever identifier they still yield (RAC-KXBPS7SRM6ZB
/// REQ-002: creation must succeed).
fn issued_ids(repository_root: &str) -> HashSet<String> {
    corpus_items(repository_root, true)
        .iter()
        .map(|item| {
            crate::identity::artifact_identifier(&item.artifact, item.spec, &item.path)
                .to_uppercase()
        })
        .collect()
}

/// `_assign_id` / migrate's `_next_id` — generate, check, retry bounded.
fn assign_id(repository_key: &str, issued: &mut HashSet<String>) -> Result<String, ScaffoldError> {
    for _ in 0..MAX_ID_ATTEMPTS {
        let candidate = generate_id(repository_key);
        let upper = candidate.to_uppercase();
        if !issued.contains(&upper) {
            issued.insert(upper);
            return Ok(candidate);
        }
    }
    Err(id_generation_exhausted())
}

/// `create_artifact(artifact_type, output_path)` — write one new artifact
/// with assigned identity. The path is taken literally: no slug derivation,
/// no directory creation, never overwrite.
pub fn create_artifact(
    artifact_type: &str,
    output_path: &str,
) -> Result<CreatedArtifact, ScaffoldError> {
    let body = load_template(artifact_type)?; // validates the type first
    if Path::new(output_path).exists() {
        return Err(ScaffoldError::OutputPathExists(format!(
            "{output_path} already exists; decided new never overwrites"
        )));
    }
    let parent = py_parent(output_path);
    if !Path::new(&parent).is_dir() {
        return Err(ScaffoldError::OutputDirectoryMissing(format!(
            "directory does not exist: {parent}"
        )));
    }
    let Some(config) = load_repository_config(&parent)? else {
        return Err(missing_repository_config(&parent));
    };
    // repository_root = str(Path(config_path).parent.parent) — the resolved
    // config path's grandparent (the directory holding `.decided/`).
    let repository_root = Path::new(&config.config_path)
        .parent()
        .and_then(Path::parent)
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|| ".".to_string());
    let mut issued = issued_ids(&repository_root);
    let artifact_id = assign_id(&config.repository_key, &mut issued)?;
    let content = format!("{}{body}", render_frontmatter(&artifact_id, artifact_type));
    std::fs::write(output_path, content.as_bytes()).map_err(|e| {
        // The oracle lets a write OSError escape as a traceback (exit 1);
        // surface the same operational class without the traceback noise.
        ScaffoldError::MalformedRepositoryConfig(format!("cannot write {output_path}: {e}"))
    })?;
    Ok(CreatedArtifact {
        artifact_type: artifact_type.to_string(),
        path: output_path.to_string(),
        id: artifact_id,
    })
}

// ---------------------------------------------------------------------------
// Quickstart (decided.services.quickstart, ADR-044)
// ---------------------------------------------------------------------------

/// Outcome of one `decided quickstart` run.
pub struct QuickstartResult {
    pub repository_key: String,
    pub config_path: String,
    pub created: bool,
    pub artifact: CreatedArtifact,
}

/// `quickstart(directory, key, artifact_type)` — validate the type first,
/// refuse a non-empty corpus BEFORE any write, establish identity, then
/// scaffold `<dir>/decisions/<type>s/first-<type>.md`.
///
/// Check order is load-bearing (measured): bad type (exit 2) beats a
/// non-empty corpus (exit 1) beats a bad key (exit 2 when reached). Note
/// the identity write lands BEFORE the starter-exists refusal, exactly like
/// the oracle (`init_repository` precedes `create_artifact`).
pub fn quickstart(
    directory: &str,
    key: &str,
    artifact_type: &str,
) -> Result<QuickstartResult, ScaffoldError> {
    load_template(artifact_type)?; // validate before any side effect

    // Refuse a non-empty corpus: any entry classified as a known type. The
    // oracle crashes when this walk hits hostile markdown; the native walk
    // is total (RAC-KXBPS7SRM6ZB REQ-002 class, documented divergence).
    let items = corpus_items(directory, true);
    if let Some(existing) = items.iter().find(|item| item.spec.is_some()) {
        return Err(ScaffoldError::CorpusNotEmpty(format!(
            "corpus already has artifacts (e.g. {}); decided quickstart only \
             scaffolds an empty corpus \u{2014} use `decided new` to add more",
            existing.path
        )));
    }

    let init_result = init_repository(directory, key, None, None, None)?;

    let family = format!("{artifact_type}s");
    let art_dir = py_join(directory, &["decisions", &family]);
    std::fs::create_dir_all(&art_dir)
        .map_err(|e| malformed_config(&art_dir, &format!("invalid YAML: {e}")))?;
    let file_name = format!("first-{artifact_type}.md");
    let out_path = py_join(directory, &["decisions", &family, &file_name]);
    let artifact = create_artifact(artifact_type, &out_path)?;

    Ok(QuickstartResult {
        repository_key: init_result.repository_key,
        config_path: init_result.config_path,
        created: init_result.created,
        artifact,
    })
}

// ---------------------------------------------------------------------------
// Metadata migration (decided.services.migrate, ADR-025)
// ---------------------------------------------------------------------------

/// Stable per-file statuses (part of the JSON contract, ADR-007).
pub const STATUS_MIGRATED: &str = "migrated";
pub const STATUS_ALREADY_CANONICAL: &str = "already-canonical";
pub const STATUS_SKIPPED_UNKNOWN: &str = "skipped-unknown";

/// Migration outcome for one Markdown file.
pub struct FileMigration {
    pub path: String,
    pub status: &'static str,
    pub id: Option<String>,
    pub artifact_type: Option<String>,
}

/// Repository-level migration result (stable JSON contract, ADR-007).
pub struct MigrationReport {
    pub directory: String,
    pub recursive: bool,
    pub dry_run: bool,
    pub files: Vec<FileMigration>,
}

impl MigrationReport {
    fn count(&self, status: &str) -> usize {
        self.files.iter().filter(|f| f.status == status).count()
    }

    pub fn migrated(&self) -> usize {
        self.count(STATUS_MIGRATED)
    }

    pub fn already_canonical(&self) -> usize {
        self.count(STATUS_ALREADY_CANONICAL)
    }

    pub fn skipped_unknown(&self) -> usize {
        self.count(STATUS_SKIPPED_UNKNOWN)
    }
}

/// `migrate_metadata(directory, dry_run, recursive)` — prepend the
/// canonical envelope to every recognized artifact without frontmatter,
/// body bytes untouched. ANY frontmatter presence — valid, malformed, or
/// unterminated — is `already-canonical` (validation owns broken
/// envelopes); documents that do not classify are `skipped-unknown`.
pub fn migrate_metadata(
    directory: &str,
    dry_run: bool,
    recursive: bool,
) -> Result<MigrationReport, ScaffoldError> {
    let Some(config) = load_repository_config(directory)? else {
        return Err(missing_repository_config(directory));
    };
    let repository_root = Path::new(&config.config_path)
        .parent()
        .and_then(Path::parent)
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|| ".".to_string());
    let mut issued = issued_ids(&repository_root);

    let mut files = Vec::new();
    for item in corpus_items(directory, recursive) {
        if item.artifact.metadata.is_some() || !item.artifact.metadata_issues.is_empty() {
            files.push(FileMigration {
                path: item.path.clone(),
                status: STATUS_ALREADY_CANONICAL,
                id: None,
                artifact_type: None,
            });
            continue;
        }
        let Some(spec) = item.spec else {
            files.push(FileMigration {
                path: item.path.clone(),
                status: STATUS_SKIPPED_UNKNOWN,
                id: None,
                artifact_type: None,
            });
            continue;
        };
        let artifact_id = assign_id(&config.repository_key, &mut issued)?;
        if !dry_run {
            // Prepend the envelope only; the body bytes are untouched.
            let original = std::fs::read(&item.path).map_err(|e| {
                malformed_config(&item.path, &format!("invalid YAML: {e}"))
            })?;
            let mut data = render_frontmatter(&artifact_id, &spec.name).into_bytes();
            data.extend_from_slice(&original);
            std::fs::write(&item.path, data).map_err(|e| {
                malformed_config(&item.path, &format!("invalid YAML: {e}"))
            })?;
        }
        files.push(FileMigration {
            path: item.path.clone(),
            status: STATUS_MIGRATED,
            id: Some(artifact_id),
            artifact_type: Some(spec.name.clone()),
        });
    }
    Ok(MigrationReport {
        directory: directory.to_string(),
        recursive,
        dry_run,
        files,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn id_shape_is_key_dash_twelve_crockford() {
        let id = generate_id("RAC");
        assert!(id.starts_with("RAC-"));
        let tail = &id[4..];
        assert_eq!(tail.len(), 12);
        assert!(tail.bytes().all(|b| ID_ALPHABET.contains(&b)), "{id}");
    }

    #[test]
    fn key_contract_edges() {
        assert!(valid_repository_key("RAC"));
        assert!(valid_repository_key("AB"));
        assert!(valid_repository_key("A234567890"));
        assert!(!valid_repository_key("A"));
        assert!(!valid_repository_key("ABCDEFGHIJK"));
        assert!(!valid_repository_key("bad"));
        assert!(!valid_repository_key("1AB"));
        // Python `$` matches just before one trailing newline.
        assert!(valid_repository_key("RAC\n"));
    }

    /// RAC-KXBPS7SRM6ZB REQ-002: the native `decided new` must succeed when the
    /// repository walk encounters unparseable/hostile Markdown. The oracle
    /// CRASHES on this fixture (an unhashable YAML mapping key — a list —
    /// raises `TypeError` inside `_no_duplicates` during the id-collision
    /// index walk, measured exit 1 with a traceback); the native walk is
    /// total, skips the hostile file's broken envelope, and mints an id.
    #[test]
    fn new_survives_hostile_markdown_in_the_walk() {
        let base = std::env::var("CARGO_TARGET_TMPDIR").unwrap_or_else(|_| "/tmp".into());
        let root = std::path::Path::new(&base)
            .join(format!("scaffold_hostile_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("decisions/decisions")).unwrap();
        std::fs::create_dir_all(root.join(".decided")).unwrap();
        std::fs::write(root.join(".decided/config.yaml"), "repository_key: RAC\n").unwrap();
        // The pinned oracle-crash repro: a YAML mapping with a LIST key.
        let hostile = format!(
            "{}/../fuzz/pinned/oracle-crashes/unhashable-key/repro.md",
            env!("CARGO_MANIFEST_DIR")
        );
        let hostile_bytes = std::fs::read(&hostile)
            .unwrap_or_else(|e| panic!("cannot read {hostile}: {e}"));
        std::fs::write(root.join("decisions/decisions/case.md"), hostile_bytes).unwrap();

        let out = root.join("decisions/decisions/new.md").to_string_lossy().into_owned();
        let created = match create_artifact("decision", &out) {
            Ok(created) => created,
            Err(e) => panic!("create_artifact failed on a hostile corpus: {}", e.message()),
        };
        assert_eq!(created.artifact_type, "decision");
        let written = std::fs::read_to_string(&out).unwrap();
        assert!(written.starts_with("---\nschema_version: 1\nid: RAC-"));
        let _ = std::fs::remove_dir_all(&root);
    }
}
