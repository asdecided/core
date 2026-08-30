//! Command orchestration: walk -> parse -> classify -> validate -> render.
//! Output is order-deterministic.

use std::collections::{BTreeSet, VecDeque};
use std::path::{Component, Path, PathBuf};

use crate::corpus::{ArtifactOrigin, CorpusLayer};
use crate::output;
use crate::parse::{parse_file, parse_text, Artifact, Issue};
use crate::relationships::{
    build_relationship_report, build_relationship_report_file, corpus_items,
    validate_document_against_corpus, validate_relationships, validate_relationships_file,
    RelationshipIssue,
};
use crate::validate::{
    apply_overrides, check_okf_conformance, has_errors, load_overrides, load_ticketing_provider,
    validate, validate_product, OkfConformanceReport, OkfEntry,
};
use crate::walk::normalize_root;

pub const EXIT_OK: i32 = 0;
pub const EXIT_VALIDATION_FAILED: i32 = 1;
pub const EXIT_USAGE: i32 = 2;

// Stable per-file statuses (JSON contract).
pub const STATUS_VALID: &str = "valid";
pub const STATUS_INVALID: &str = "invalid";
pub const STATUS_SKIPPED: &str = "skipped";

fn usage_error(message: &str) -> i32 {
    eprintln!("decided: {message}");
    EXIT_USAGE
}

fn emit(text: String) {
    use std::io::Write;
    // stdin surrogateescape sentinels re-materialize as their raw bytes on
    // stdout (the oracle's stdout encoder uses surrogateescape). No-op —
    // a borrowed passthrough — unless stdin decoding produced sentinels.
    let payload = crate::pycompat::encode_stdout_surrogateescape(&text);
    let mut stdout = std::io::stdout().lock();
    let _ = stdout.write_all(&payload);
    let _ = stdout.write_all(b"\n");
    let _ = stdout.flush();
}

fn emit_exact(text: &str) {
    use std::io::Write;
    let payload = crate::pycompat::encode_stdout_surrogateescape(text);
    let mut stdout = std::io::stdout().lock();
    let _ = stdout.write_all(&payload);
    let _ = stdout.flush();
}

// ---------------------------------------------------------------------------
// Service results (decided.services.validate)
// ---------------------------------------------------------------------------

pub struct FileValidation {
    pub path: String,
    pub artifact_type: String,
    pub status: &'static str,
    pub issues: Vec<Issue>,
    /// Stable source and layer identity for rows built from a composed corpus.
    /// Released single-corpus validation leaves this absent so its rendered
    /// output remains byte-identical.
    pub origin: Option<ArtifactOrigin>,
    /// Canonical source route for graph topology findings only. Ordinary
    /// artifact validation and all released version-1 paths leave this absent.
    pub source_route: Option<Vec<String>>,
    /// Exact verified physical-route count represented by `source_route`.
    pub route_count: Option<usize>,
}

pub struct DirectoryValidation {
    pub directory: String,
    pub recursive: bool,
    pub files: Vec<FileValidation>,
    pub okf: Option<OkfConformanceReport>,
}

impl DirectoryValidation {
    pub fn checked(&self) -> usize {
        self.files.iter().filter(|f| f.status != STATUS_SKIPPED).count()
    }

    pub fn valid(&self) -> usize {
        self.files.iter().filter(|f| f.status == STATUS_VALID).count()
    }

    pub fn invalid(&self) -> usize {
        self.files.iter().filter(|f| f.status == STATUS_INVALID).count()
    }

    pub fn skipped(&self) -> usize {
        self.files.iter().filter(|f| f.status == STATUS_SKIPPED).count()
    }

    pub fn ok(&self) -> bool {
        self.invalid() == 0 && self.okf.as_ref().map(|o| o.ok()).unwrap_or(true)
    }
}

pub struct StdinCorpusValidation {
    pub source_path: String,
    pub structural_issues: Vec<Issue>,
    pub relationship_issues: Vec<RelationshipIssue>,
}

impl StdinCorpusValidation {
    pub fn ok(&self) -> bool {
        !has_errors(&self.structural_issues) && self.relationship_issues.is_empty()
    }
}

/// `validate_directory(directory, recursive)` — the uncached walk (the cache
/// path is contractually byte-identical, PORT-CONTRACT.d/01 §6).
pub fn validate_directory(directory: &str, recursive: bool) -> DirectoryValidation {
    let entries = corpus_items(directory, recursive);
    let overrides = load_overrides(directory);
    let provider = load_ticketing_provider(directory);
    // Per-file validation in parallel over the sorted corpus (PORT-CONTRACT
    // decision 5): an indexed rayon iterator, so `collect` preserves the
    // sorted order and the worker count is invisible in the output. The
    // shared inputs (overrides, provider) are read-only.
    use rayon::prelude::*;
    let files: Vec<FileValidation> = entries
        .par_iter()
        .map(|item| {
            let artifact_type = item
                .spec
                .map(|s| s.name.clone())
                .unwrap_or_else(|| "unknown".to_string());
            if item.spec.is_none() {
                return FileValidation {
                    path: item.path.clone(),
                    artifact_type,
                    status: STATUS_SKIPPED,
                    issues: Vec::new(),
                    origin: None,
                    source_route: None,
                    route_count: None,
                };
            }
            let issues = apply_overrides(
                validate(&item.artifact, provider.as_deref(), Some(&artifact_type)),
                &artifact_type,
                &overrides,
            );
            let status = if has_errors(&issues) {
                STATUS_INVALID
            } else {
                STATUS_VALID
            };
            FileValidation {
                path: item.path.clone(),
                artifact_type,
                status,
                issues,
                origin: None,
                source_route: None,
                route_count: None,
            }
        })
        .collect();
    let okf_entries: Vec<OkfEntry> = entries
        .iter()
        .filter(|item| item.origin.layer == crate::corpus::Layer::Local)
        .map(|item| OkfEntry {
            path: &item.path,
            artifact_type: item
                .spec
                .map(|s| s.name.as_str())
                .unwrap_or("unknown"),
            file_name: item.path.rsplit('/').next().unwrap_or(&item.path),
        })
        .collect();
    let okf = check_okf_conformance(&okf_entries, &overrides);
    DirectoryValidation {
        directory: directory.to_string(),
        recursive,
        files,
        okf: Some(okf),
    }
}

/// Structural validation over the effective projection of one already-loaded
/// composition. Inherited errors were collapsed by the loader; their warnings
/// remain parent-owned and are not repeated in every child.
pub(crate) fn validate_directory_from_items(
    directory: &str,
    recursive: bool,
    entries: &[crate::relationships::CorpusItem],
) -> DirectoryValidation {
    let overrides = load_overrides(directory);
    let provider = load_ticketing_provider(directory);
    use rayon::prelude::*;
    let files: Vec<FileValidation> = entries
        .par_iter()
        .map(|item| {
            let artifact_type = item
                .spec
                .map(|spec| spec.name.clone())
                .unwrap_or_else(|| "unknown".to_string());
            if item.spec.is_none() {
                return FileValidation {
                    path: item.path.clone(),
                    artifact_type,
                    status: STATUS_SKIPPED,
                    issues: Vec::new(),
                    origin: Some(item.origin.clone()),
                    source_route: None,
                    route_count: None,
                };
            }
            let issues = if item.origin.layer == crate::corpus::Layer::Inherited {
                Vec::new()
            } else {
                apply_overrides(
                    validate(&item.artifact, provider.as_deref(), Some(&artifact_type)),
                    &artifact_type,
                    &overrides,
                )
            };
            let status = if has_errors(&issues) {
                STATUS_INVALID
            } else {
                STATUS_VALID
            };
            FileValidation {
                path: item.path.clone(),
                artifact_type,
                status,
                issues,
                origin: Some(item.origin.clone()),
                source_route: None,
                route_count: None,
            }
        })
        .collect();
    let okf_entries: Vec<OkfEntry> = entries
        .iter()
        .filter(|item| item.origin.layer == crate::corpus::Layer::Local)
        .map(|item| OkfEntry {
            path: &item.path,
            artifact_type: item.spec.map(|spec| spec.name.as_str()).unwrap_or("unknown"),
            file_name: item.path.rsplit('/').next().unwrap_or(&item.path),
        })
        .collect();
    DirectoryValidation {
        directory: directory.to_string(),
        recursive,
        files,
        okf: Some(check_okf_conformance(&okf_entries, &overrides)),
    }
}

fn load_composed_or_exit(
    directory: &str,
    recursive: bool,
) -> Result<Option<crate::composition::ComposedCorpus>, i32> {
    load_composed_or_exit_with_boundary(directory, recursive, None)
}

fn load_composed_or_exit_with_boundary(
    directory: &str,
    recursive: bool,
    boundary: Option<&Path>,
) -> Result<Option<crate::composition::ComposedCorpus>, i32> {
    let loaded = match boundary {
        Some(root) => crate::federated_corpus::load_composed_corpus_with_boundary(
            directory,
            recursive,
            root,
        ),
        None => crate::federated_corpus::load_composed_corpus(directory, recursive),
    };
    match loaded {
        Ok(corpus) => Ok(corpus),
        Err(error) => {
            eprintln!("decided: {error}");
            Err(EXIT_VALIDATION_FAILED)
        }
    }
}

fn refuse_read_only_target(path: &str) -> Option<i32> {
    match crate::federated_corpus::is_read_only_materialised_path(path) {
        Ok(false) => None,
        Ok(true) => {
            eprintln!(
                "decided: refusing to write inside the inherited read-only parent materialisation: {path}"
            );
            Some(EXIT_VALIDATION_FAILED)
        }
        Err(error) => {
            eprintln!("decided: {error}");
            Some(EXIT_VALIDATION_FAILED)
        }
    }
}

fn refuse_read_only_targets<'a>(paths: impl IntoIterator<Item = &'a str>) -> Option<i32> {
    for path in paths {
        if let Some(code) = refuse_read_only_target(path) {
            return Some(code);
        }
    }
    None
}

/// A fingerprint of the ancestor-walked `.decided/config.yaml` governing
/// `directory` — the per-file cache key's config half (ADR-106).
fn config_fingerprint(directory: &str) -> String {
    let mut hasher = crate::sha256::Sha256::new();
    match crate::validate::find_config_file(directory) {
        None => hasher.update(b"\x00no-config"),
        Some(config_path) => {
            hasher.update(config_path.display().to_string().as_bytes());
            hasher.update(b"\0");
            match std::fs::read(&config_path) {
                Ok(bytes) => hasher.update(&bytes),
                Err(_) => hasher.update(b"\x00unreadable-config"),
            }
        }
    }
    hasher.hexdigest()
}

/// A stable per-corpus-root store key: SHA-256 of the resolved path.
fn validate_root_key(directory: &str) -> String {
    let resolved = crate::index_store::py_resolve(directory);
    crate::sha256::hexdigest(resolved.display().to_string().as_bytes())
}

/// `validate_directory_incremental(directory, recursive, verify)` — the
/// ADR-106 changeset-bound path, byte-identical to `validate_directory` for
/// the same corpus and config. Unchanged files reuse their cached path-free
/// result verbatim; changed files re-parse and re-validate; assembly runs in
/// walk order; OKF conformance recomputes over `(artifact_type, path)` shims.
pub fn validate_directory_incremental(
    directory: &str,
    recursive: bool,
    verify: bool,
) -> DirectoryValidation {
    validate_directory_incremental_in(directory, recursive, verify, None)
}

/// The cache-dir-injectable body (`cache_dir=None` resolves the ladder) —
/// the seam the S5 pinning test drives without touching process env.
pub fn validate_directory_incremental_in(
    directory: &str,
    recursive: bool,
    verify: bool,
    cache_dir: Option<&Path>,
) -> DirectoryValidation {
    use crate::index_store::{
        open_validation_store, write_validation_store, FileState, ValidationCacheRow,
    };
    let timing = std::env::var_os("DECIDED_TIMING").is_some();
    let cache_dir = cache_dir
        .map(Path::to_path_buf)
        .unwrap_or_else(crate::derived_cache::default_cache_dir);
    let root_key = validate_root_key(directory);
    let config_hash = config_fingerprint(directory);

    let prev_rows =
        open_validation_store(&cache_dir, &root_key, &config_hash).unwrap_or_default();
    let prev_manifest: Vec<(String, FileState)> = prev_rows
        .iter()
        .map(|(rel, row)| {
            (
                rel.clone(),
                FileState {
                    content_hash: row.content_hash.clone(),
                    size: row.size,
                    mtime_ns: row.mtime_ns,
                },
            )
        })
        .collect();
    let prev_by_rel: std::collections::HashMap<&str, &ValidationCacheRow> = prev_rows
        .iter()
        .map(|(rel, row)| (rel.as_str(), row))
        .collect();

    let detect_start = std::time::Instant::now();
    let (new_manifest, changed) =
        crate::derived_cache::stat_scan(directory, &prev_manifest, verify, recursive);
    let detect_ms = detect_start.elapsed().as_secs_f64() * 1000.0;

    let overrides = load_overrides(directory);
    let provider = load_ticketing_provider(directory);
    let root_display = normalize_root(directory);

    let recompute_start = std::time::Instant::now();
    let mut new_rows: Vec<(String, ValidationCacheRow)> =
        Vec::with_capacity(new_manifest.len());
    for (rel, state) in &new_manifest {
        if !changed.contains(rel) {
            if let Some(prev) = prev_by_rel.get(rel.as_str()) {
                // Unchanged content under an unchanged config: reuse the
                // path-free result verbatim, refreshing only the stat proxy.
                new_rows.push((
                    rel.clone(),
                    ValidationCacheRow {
                        size: state.size,
                        mtime_ns: state.mtime_ns,
                        content_hash: state.content_hash.clone(),
                        artifact_type: prev.artifact_type.clone(),
                        status: prev.status.clone(),
                        issues: prev.issues.clone(),
                    },
                ));
                continue;
            }
        }
        let path = format!("{root_display}/{rel}");
        let artifact = parse_file(&path);
        let spec = crate::spec::spec_for(&crate::classify::classify(&artifact).artifact_type);
        let artifact_type = spec
            .map(|s| s.name.clone())
            .unwrap_or_else(|| "unknown".to_string());
        let (status, issues) = if spec.is_none() {
            (STATUS_SKIPPED.to_string(), Vec::new())
        } else {
            let computed = apply_overrides(
                validate(&artifact, provider.as_deref(), Some(&artifact_type)),
                &artifact_type,
                &overrides,
            );
            let status = if has_errors(&computed) {
                STATUS_INVALID
            } else {
                STATUS_VALID
            };
            (
                status.to_string(),
                computed
                    .into_iter()
                    .map(|issue| crate::index_store::CachedIssue {
                        severity: issue.severity.to_string(),
                        code: issue.code.clone(),
                        message: issue.message.clone(),
                        line: issue.line.map(|l| l as u32),
                    })
                    .collect(),
            )
        };
        new_rows.push((
            rel.clone(),
            ValidationCacheRow {
                size: state.size,
                mtime_ns: state.mtime_ns,
                content_hash: state.content_hash.clone(),
                artifact_type,
                status,
                issues,
            },
        ));
    }
    let recompute_ms = recompute_start.elapsed().as_secs_f64() * 1000.0;

    // Assemble in walk order — byte-identical file and issue order.
    let rows_by_rel: std::collections::HashMap<&str, &ValidationCacheRow> = new_rows
        .iter()
        .map(|(rel, row)| (rel.as_str(), row))
        .collect();
    let mut files: Vec<FileValidation> = Vec::new();
    let mut okf_entries_owned: Vec<(String, String, String)> = Vec::new();
    for entry in crate::walk::find_markdown_files(directory, recursive) {
        let rel = entry.components.join("/");
        let Some(row) = rows_by_rel.get(rel.as_str()) else {
            continue; // created between scan and assembly — next run settles it
        };
        let status: &'static str = match row.status.as_str() {
            "valid" => STATUS_VALID,
            "invalid" => STATUS_INVALID,
            _ => STATUS_SKIPPED,
        };
        files.push(FileValidation {
            path: entry.display.clone(),
            artifact_type: row.artifact_type.clone(),
            status,
            issues: row
                .issues
                .iter()
                .map(|i| Issue {
                    severity: match i.severity.as_str() {
                        "error" => "error",
                        "warning" => "warning",
                        _ => "info",
                    },
                    code: i.code.clone(),
                    message: i.message.clone(),
                    line: i.line.map(i64::from),
                })
                .collect(),
            origin: None,
            source_route: None,
            route_count: None,
        });
        let file_name = entry
            .display
            .rsplit('/')
            .next()
            .unwrap_or(&entry.display)
            .to_string();
        okf_entries_owned.push((entry.display.clone(), row.artifact_type.clone(), file_name));
    }
    let okf_entries: Vec<OkfEntry> = okf_entries_owned
        .iter()
        .map(|(path, artifact_type, file_name)| OkfEntry {
            path,
            artifact_type,
            file_name,
        })
        .collect();
    let okf = check_okf_conformance(&okf_entries, &overrides);

    write_validation_store(&cache_dir, &root_key, &config_hash, &new_rows);

    if timing {
        eprintln!(
            "decided-timing: detect_ms={detect_ms:.3} recompute_ms={recompute_ms:.3} files_changed={}",
            changed.len()
        );
    }

    DirectoryValidation {
        directory: directory.to_string(),
        recursive,
        files,
        okf: Some(okf),
    }
}

/// `validate_stdin_against_corpus(product, corpus_dir, source_path)`.
pub fn validate_stdin_against_corpus(
    artifact: &Artifact,
    corpus_dir: &str,
    source_path: &str,
    recursive: bool,
) -> StdinCorpusValidation {
    let structural = validate_product(artifact, corpus_dir);
    let relationships =
        validate_document_against_corpus(artifact, source_path, corpus_dir, recursive);
    StdinCorpusValidation {
        source_path: source_path.to_string(),
        structural_issues: structural,
        relationship_issues: relationships.issues,
    }
}

fn validate_stdin_against_composed(
    artifact: &Artifact,
    corpus_dir: &str,
    source_path: &str,
    recursive: bool,
    corpus: &crate::composition::ComposedCorpus,
) -> StdinCorpusValidation {
    let structural = validate_product(artifact, corpus_dir);
    let relationships =
        corpus.validate_proposed_document(artifact, source_path, corpus_dir, recursive);
    StdinCorpusValidation {
        source_path: source_path.to_string(),
        structural_issues: structural,
        relationship_issues: relationships.issues,
    }
}

// ---------------------------------------------------------------------------
// cmd_validate
// ---------------------------------------------------------------------------

pub struct ValidateArgs {
    pub file: String,
    pub json: bool,
    pub sarif: bool,
    pub top_level: bool,
    pub corpus: Option<String>,
    /// `--cache` / `--no-cache` (ADR-112: on by default).
    pub cache: bool,
    /// `--verify`: full content re-hash of the cache freshness check.
    pub verify: bool,
}

/// `str(Path(p))` — PurePosixPath normalization of a CLI path argument.
fn py_path_str(p: &str) -> String {
    normalize_root(p)
}

/// `str(Path(p).parent)`.
fn py_path_parent(p: &str) -> String {
    let normalized = py_path_str(p);
    if normalized == "/" || normalized == "." {
        return normalized;
    }
    match normalized.rfind('/') {
        Some(0) => "/".to_string(),
        Some(i) => normalized[..i].to_string(),
        None => ".".to_string(),
    }
}

/// `_read(path)` — a directly named file that is missing or unreadable is a
/// usage error. Returns Err(exit_code) on usage failure.
fn read_named_file(path: &str) -> Result<Artifact, i32> {
    if !Path::new(path).is_file() {
        return Err(usage_error(&format!("file not found: {path}")));
    }
    let artifact = parse_file(path);
    if artifact
        .parse_issues
        .iter()
        .any(|i| i.code == "unreadable-artifact")
    {
        return Err(usage_error(&format!("cannot read {path}")));
    }
    Ok(artifact)
}

fn read_validate_input(target: &str) -> Result<Artifact, i32> {
    if target == "-" {
        use std::io::Read;
        let mut buf = Vec::new();
        let _ = std::io::stdin().lock().read_to_end(&mut buf);
        // The oracle reads stdin as TEXT with errors="surrogateescape" —
        // NOT the errors="replace" lossy decode used for files.
        let text = crate::pycompat::decode_stdin_surrogateescape(&buf);
        return Ok(parse_text(&text, "-"));
    }
    read_named_file(target)
}

/// Recover the declared inherited identity for a composed-corpus load error.
/// A malformed or unreadable manifest has no trustworthy provenance, while a
/// successfully parsed declaration can still identify failures from later
/// materialisation, pin, or composition checks.
fn manifest_failure_origin(directory: &str) -> Option<ArtifactOrigin> {
    let repository_root = crate::validate::repository_root(directory);
    let manifest = crate::federation::load_manifest(&repository_root)
        .ok()
        .flatten()?;
    Some(
        CorpusLayer::inherited(
            manifest.inherits.source,
            manifest.inherits.alias,
            manifest.inherits.digest,
        )
        .origin(),
    )
}

pub fn cmd_validate(args: &ValidateArgs) -> i32 {
    // Directory? Validate every recognized artifact beneath it.
    if args.file != "-" && Path::new(&args.file).is_dir() {
        if args.corpus.is_some() {
            return usage_error("--corpus applies to stdin ('-') or a single file");
        }
        let composed = crate::federated_corpus::load_composed_corpus(
            &args.file,
            !args.top_level,
        );
        // The cache reuses per-file results across runs (ADR-106),
        // byte-identical to the uncached path; on by default per ADR-112.
        let result = match composed {
            Ok(Some(composed)) => {
                let items: Vec<_> = composed.effective().cloned().collect();
                validate_directory_from_items(&args.file, !args.top_level, &items)
            }
            Ok(None) if crate::derived_cache::cache_enabled(args.cache) => {
                validate_directory_incremental(&args.file, !args.top_level, args.verify)
            }
            Ok(None) => validate_directory(&args.file, !args.top_level),
            Err(error) => {
                let origin = error
                    .validation_origin()
                    .map(|origin| ArtifactOrigin {
                        source: origin.source.clone(),
                        layer: origin.layer,
                        pin: origin.pin.clone(),
                        alias: None,
                    })
                    .or_else(|| manifest_failure_origin(&args.file));
                let source_route = error.source_route().map(<[String]>::to_vec);
                let route_count = error.route_count();
                DirectoryValidation {
                    directory: args.file.clone(),
                    recursive: !args.top_level,
                    files: vec![FileValidation {
                        path: crate::federation::MANIFEST_RELATIVE_PATH.to_string(),
                        artifact_type: "corpus-manifest".to_string(),
                        status: STATUS_INVALID,
                        issues: vec![Issue::new(
                            "error",
                            error.stable_code(),
                            error.to_string(),
                            None,
                        )],
                        origin,
                        source_route,
                        route_count,
                    }],
                    okf: None,
                }
            }
        };
        if args.sarif {
            emit(output::render_validate_sarif(&result));
        } else if args.json {
            emit(output::render_validate_dir_json(&result));
        } else {
            emit(output::render_validate_dir_human(&result));
        }
        return if result.ok() {
            EXIT_OK
        } else {
            EXIT_VALIDATION_FAILED
        };
    }

    if args.sarif {
        return usage_error("--sarif applies to directory validation");
    }

    let artifact = match read_validate_input(&args.file) {
        Ok(a) => a,
        Err(code) => return code,
    };

    if let Some(corpus) = &args.corpus {
        if !Path::new(corpus).is_dir() {
            return usage_error(&format!("--corpus is not a directory: {corpus}"));
        }
        let source_path = if args.file == "-" {
            "-".to_string()
        } else {
            py_path_str(&args.file)
        };
        let result = match load_composed_or_exit(corpus, true) {
            Ok(Some(composed)) => validate_stdin_against_composed(
                &artifact,
                corpus,
                &source_path,
                true,
                &composed,
            ),
            Ok(None) => validate_stdin_against_corpus(&artifact, corpus, &source_path, true),
            Err(code) => return code,
        };
        if args.json {
            emit(output::render_stdin_corpus_json(&result));
        } else {
            emit(output::render_stdin_corpus_human(&result));
        }
        return if result.ok() {
            EXIT_OK
        } else {
            EXIT_VALIDATION_FAILED
        };
    }

    let start = if args.file == "-" {
        ".".to_string()
    } else {
        py_path_parent(&args.file)
    };
    let issues = validate_product(&artifact, &start);
    if args.json {
        emit(output::render_validation_json(
            &artifact.product.source_path,
            &issues,
        ));
    } else {
        emit(output::render_validation_human(
            &artifact.product.source_path,
            &issues,
        ));
    }
    if has_errors(&issues) {
        EXIT_VALIDATION_FAILED
    } else {
        EXIT_OK
    }
}

// ---------------------------------------------------------------------------
// cmd_diff
// ---------------------------------------------------------------------------

pub struct DiffArgs {
    pub old: String,
    pub new: String,
    pub json: bool,
}

pub fn cmd_diff(args: &DiffArgs) -> i32 {
    // `old` is `_read()` before `new`, so a bad old path wins the error.
    let old = match read_named_file(&args.old) {
        Ok(a) => a,
        Err(code) => return code,
    };
    let new = match read_named_file(&args.new) {
        Ok(a) => a,
        Err(code) => return code,
    };
    let result = crate::diff::diff(&old, &new);
    if args.json {
        emit(output::render_diff_json(&result, &args.old, &args.new));
    } else {
        emit(output::render_diff_human(&result));
    }
    EXIT_OK
}

// ---------------------------------------------------------------------------
// cmd_inspect / cmd_improve
// ---------------------------------------------------------------------------

/// `Path(target).suffix.lower()` — the final `.`-suffix of the last path
/// component, empty for dotless names, leading-dot names, and trailing dots.
fn py_suffix_lower(target: &str) -> String {
    let name = target.rsplit('/').next().unwrap_or(target);
    match name.rfind('.') {
        Some(i) if i > 0 && i < name.len() - 1 => name[i..].to_lowercase(),
        _ => String::new(),
    }
}

/// `_read_markdown_input(target, command)` — a Markdown file or stdin (`-`).
fn read_markdown_input(target: &str, command: &str) -> Result<String, i32> {
    if target == "-" {
        use std::io::Read;
        let mut buf = Vec::new();
        let _ = std::io::stdin().lock().read_to_end(&mut buf);
        // `sys.stdin.read()` under the harness locale decodes UTF-8 with
        // errors="surrogateescape" — same seam as `validate -`.
        return Ok(crate::pycompat::decode_stdin_surrogateescape(&buf));
    }
    if !Path::new(target).is_file() {
        return Err(usage_error(&format!("file not found: {target}")));
    }
    let suffix = py_suffix_lower(target);
    if suffix != ".md" && suffix != ".markdown" {
        return Err(usage_error(&format!(
            "{command} expects a Markdown file; convert {target} first with an AsDecided ingestion connector"
        )));
    }
    match std::fs::read(target) {
        Ok(bytes) => match String::from_utf8(bytes) {
            Ok(text) => Ok(text),
            // The oracle's `path.read_text(encoding="utf-8")` decodes
            // strictly: invalid UTF-8 raises UnicodeDecodeError, which no
            // handler catches — an unhandled traceback, exit 1, empty stdout.
            Err(e) => {
                eprintln!(
                    "UnicodeDecodeError: 'utf-8' codec can't decode input: {e}"
                );
                Err(EXIT_VALIDATION_FAILED)
            }
        },
        // OSError -> `decided: cannot read <t>: <err>`, exit 2.
        Err(e) => Err(usage_error(&format!("cannot read {target}: {e}"))),
    }
}

pub struct InspectArgs {
    pub file: String,
    pub verbose: bool,
    pub top_level: bool,
    pub json: bool,
}

pub fn cmd_inspect(args: &InspectArgs) -> i32 {
    if args.file != "-" {
        match crate::federated_corpus::is_read_only_graph_materialised_path(&args.file) {
            Ok(true) => {
                return usage_error(&format!(
                    "inspect in a version-2 federation is limited to root-local artifacts: {}",
                    args.file
                ))
            }
            Ok(false) => {}
            Err(error) => {
                eprintln!("decided: {error}");
                return EXIT_VALIDATION_FAILED;
            }
        }
    }
    // Directory? Aggregate per-file results into type counts. (The directory
    // check precedes the .md extension guard — and never applies to `-`.)
    if args.file != "-" && Path::new(&args.file).is_dir() {
        let result = match crate::federated_corpus::load_graph_composed_corpus(
            &args.file,
            !args.top_level,
        ) {
            Ok(Some(composed)) => {
                let local = crate::federated_corpus::local_writable_projection(
                    &args.file,
                    &composed,
                );
                crate::inspect::inspect_directory_from_items(
                    &args.file,
                    !args.top_level,
                    &local,
                )
            }
            Ok(None) => crate::inspect::inspect_directory(&args.file, !args.top_level),
            Err(error) => {
                eprintln!("decided: {error}");
                return EXIT_VALIDATION_FAILED;
            }
        };
        if args.json {
            emit(output::render_dir_inspect_json(&result));
        } else {
            emit(output::render_dir_inspect_human(&result));
        }
        return EXIT_OK;
    }

    // Single file (or stdin).
    let text = match read_markdown_input(&args.file, "inspect") {
        Ok(t) => t,
        Err(code) => return code,
    };
    let graph_artifact = if args.file == "-" {
        None
    } else {
        let target = match std::fs::canonicalize(&args.file) {
            Ok(target) => target,
            Err(error) => return usage_error(&format!("cannot read {}: {error}", args.file)),
        };
        let containing = Path::new(&args.file).parent().unwrap_or_else(|| Path::new("."));
        match crate::federated_corpus::load_graph_composed_corpus(
            &containing.to_string_lossy(),
            true,
        ) {
            Ok(Some(composed)) => match composed
                .local_items()
                .find(|item| item.locator.path == target)
            {
                Some(item) => Some(item.artifact.clone()),
                None => {
                    return usage_error(&format!(
                        "inspect in a version-2 federation is limited to root-local artifacts: {}",
                        args.file
                    ))
                }
            },
            Ok(None) => None,
            Err(error) => {
                eprintln!("decided: {error}");
                return EXIT_VALIDATION_FAILED;
            }
        }
    };
    let artifact = graph_artifact.unwrap_or_else(|| parse_text(&text, ""));
    let inspection = crate::inspect::build_inspection(&artifact);
    if args.verbose && !args.json {
        emit(output::render_inspect_verbose(
            &inspection,
            &crate::classify::score_artifacts(&artifact),
        ));
    } else if args.json {
        emit(output::render_inspect_json(&inspection));
    } else {
        emit(output::render_inspect_human(&inspection));
    }
    // A completed inspection always succeeds — Unknown is a valid outcome.
    EXIT_OK
}

pub struct ImproveArgs {
    pub file: String,
    pub json: bool,
    pub template: bool,
}

pub fn cmd_improve(args: &ImproveArgs) -> i32 {
    let text = match read_markdown_input(&args.file, "improve") {
        Ok(t) => t,
        Err(code) => return code,
    };
    let result = crate::improve::improve_product(&parse_text(&text, ""));
    if args.json {
        emit(output::render_improve_json(&result));
    } else if args.template {
        emit(output::render_improve_template(&result));
    } else {
        emit(output::render_improve_human(&result));
    }
    // Advisory: a completed analysis always succeeds.
    EXIT_OK
}

// ---------------------------------------------------------------------------
// cmd_relationships (--validate arm; inspection arm is out of this phase)
// ---------------------------------------------------------------------------

pub struct RelationshipsArgs {
    pub path: String,
    pub validate: bool,
    pub sarif: bool,
    pub json: bool,
    pub top_level: bool,
}

pub fn cmd_relationships(args: &RelationshipsArgs) -> i32 {
    if args.sarif && !args.validate {
        return usage_error("relationships --sarif requires --validate");
    }
    let path = Path::new(&args.path);
    let is_dir = if path.is_dir() {
        true
    } else if path.is_file() {
        let suffix = args
            .path
            .rsplit('/')
            .next()
            .and_then(|name| name.rfind('.').map(|i| name[i..].to_lowercase()))
            .unwrap_or_default();
        if suffix != ".md" && suffix != ".markdown" {
            return usage_error(&format!(
                "relationships expects a Markdown file or directory: {}; \
                 convert it first with an AsDecided ingestion connector",
                args.path
            ));
        }
        false
    } else {
        return usage_error(&format!("path not found: {}", args.path));
    };

    if args.validate {
        let report = if is_dir {
            match load_composed_or_exit(&args.path, !args.top_level) {
                Ok(Some(composed)) => {
                    composed.validate_relationships(&args.path, !args.top_level)
                }
                Ok(None) => validate_relationships(&args.path, !args.top_level),
                Err(code) => return code,
            }
        } else {
            validate_relationships_file(&args.path)
        };
        if args.sarif {
            emit(output::render_relationships_sarif(&report));
        } else if args.json {
            emit(output::render_relationship_validation_json(&report));
        } else {
            emit(output::render_relationship_validation_human(&report));
        }
        return if report.ok() {
            EXIT_OK
        } else {
            EXIT_VALIDATION_FAILED
        };
    }

    // Inspection arm (non --validate): always exit 0.
    let report = if is_dir {
        match load_composed_or_exit(&args.path, !args.top_level) {
            Ok(Some(composed)) => crate::relationships::build_relationship_report_from_composed(
                &args.path,
                !args.top_level,
                &composed,
            ),
            Ok(None) => build_relationship_report(&args.path, !args.top_level),
            Err(code) => return code,
        }
    } else {
        build_relationship_report_file(&args.path)
    };
    if args.json {
        emit(output::render_relationships_json(&report));
    } else {
        emit(output::render_relationships_human(&report));
    }
    EXIT_OK
}

// ---------------------------------------------------------------------------
// cmd_stats
// ---------------------------------------------------------------------------

pub struct StatsArgs {
    pub directory: String,
    pub json: bool,
}

pub fn cmd_stats(args: &StatsArgs) -> i32 {
    if !Path::new(&args.directory).is_dir() {
        return usage_error(&format!("not a directory: {}", args.directory));
    }
    let stats = match load_composed_or_exit(&args.directory, true) {
        Ok(Some(composed)) => {
            let items: Vec<_> = composed.effective().cloned().collect();
            crate::stats::collect_stats_from_items(&args.directory, &items)
        }
        Ok(None) => crate::stats::collect_stats(&args.directory),
        Err(code) => return code,
    };
    if args.json {
        emit(output::render_stats_json(&stats));
    } else {
        emit(output::render_stats_human(&stats));
    }
    if stats.has_meaningful_content() || stats.is_empty() {
        EXIT_OK
    } else {
        EXIT_VALIDATION_FAILED
    }
}

// ---------------------------------------------------------------------------
// cmd_portfolio
// ---------------------------------------------------------------------------

pub struct PortfolioArgs {
    pub directory: String,
    pub json: bool,
    pub top_level: bool,
}

pub fn cmd_portfolio(args: &PortfolioArgs) -> i32 {
    if !Path::new(&args.directory).is_dir() {
        return usage_error(&format!("not a directory: {}", args.directory));
    }
    let recursive = !args.top_level;
    let summary = match load_composed_or_exit(&args.directory, recursive) {
        Ok(Some(composed)) => {
            crate::portfolio::portfolio_from_composed(&args.directory, &composed, recursive)
        }
        Ok(None) => {
            let items = corpus_items(&args.directory, recursive);
            crate::portfolio::portfolio_from_corpus(&args.directory, &items, recursive)
        }
        Err(code) => return code,
    };
    if args.json {
        emit(output::render_portfolio_json(&summary));
    } else {
        emit(output::render_portfolio_human(&summary));
    }
    EXIT_OK
}

// ---------------------------------------------------------------------------
// cmd_index
// ---------------------------------------------------------------------------

pub struct IndexArgs {
    pub directory: String,
    pub json: bool,
    pub top_level: bool,
}

/// `decided index` — the plain-walk inventory; never touches the cache.
pub fn cmd_index(args: &IndexArgs) -> i32 {
    if !Path::new(&args.directory).is_dir() {
        return usage_error(&format!("not a directory: {}", args.directory));
    }
    let recursive = !args.top_level;
    let index = match load_composed_or_exit(&args.directory, recursive) {
        Ok(Some(composed)) => {
            let items: Vec<_> = composed.effective().cloned().collect();
            crate::index::build_repository_index_from_items(&args.directory, &items, recursive)
        }
        Ok(None) => crate::index::build_repository_index(&args.directory, recursive),
        Err(code) => return code,
    };
    if args.json {
        emit(output::render_index_json(&index));
    } else {
        emit(output::render_index_human(&index));
    }
    EXIT_OK
}

// ---------------------------------------------------------------------------
// cmd_coverage
// ---------------------------------------------------------------------------

pub struct CoverageArgs {
    pub directory: String,
    pub json: bool,
}

/// Advisory, never a build failure: exit 0 on every valid run (REQ-005).
pub fn cmd_coverage(args: &CoverageArgs) -> i32 {
    if !Path::new(&args.directory).is_dir() {
        return usage_error(&format!("not a directory: {}", args.directory));
    }
    let report = match load_composed_or_exit(&args.directory, true) {
        Ok(Some(composed)) => {
            crate::coverage::analyze_coverage_from_composed(&args.directory, &composed)
        }
        Ok(None) => crate::coverage::analyze_coverage(&args.directory),
        Err(code) => return code,
    };
    if args.json {
        emit(output::render_coverage_json(&report));
    } else {
        emit(output::render_coverage_human(&report));
    }
    EXIT_OK
}

// ---------------------------------------------------------------------------
// cmd_decisions_for
// ---------------------------------------------------------------------------

pub struct DecisionsForArgs {
    pub path: String,
    pub directory: String,
    pub json: bool,
    pub top_level: bool,
}

/// A query always succeeds: governed, ungoverned, and outside-repository
/// paths all exit 0 (REQ-004); only a bad corpus directory is a usage error.
pub fn cmd_decisions_for(args: &DecisionsForArgs) -> i32 {
    if !Path::new(&args.directory).is_dir() {
        return usage_error(&format!("not a directory: {}", args.directory));
    }
    let composed = match load_composed_or_exit(&args.directory, !args.top_level) {
        Ok(composed) => composed,
        Err(code) => return code,
    };
    let result = if let Some(composed) = &composed {
        let items: Vec<_> = composed.effective().cloned().collect();
        let rows = crate::retrieve::scope_rows_from_items(&items);
        crate::retrieve::decisions_for_path_with_rows(&rows, &args.directory, &args.path)
    } else {
        crate::retrieve::decisions_for_path(&args.directory, &args.path, !args.top_level)
    };
    if args.json {
        emit(if let Some(composed) = &composed {
            output::render_decisions_for_json_with_composed(&result, composed)
        } else {
            output::render_decisions_for_json(&result)
        });
    } else {
        emit(output::render_decisions_for_human(&result));
    }
    EXIT_OK
}

// ---------------------------------------------------------------------------
// cmd_gate
// ---------------------------------------------------------------------------

pub struct GateArgs {
    pub directory: String,
    pub json: bool,
    pub sarif: bool,
    pub top_level: bool,
    pub code: bool,
    pub repository: String,
    pub base: Option<String>,
    pub full: bool,
}

/// One enforcement entry point: validation + relationships + review under
/// the corpus policy. Blocking findings fail (exit 1); a malformed
/// `.decided/config.yaml` is an operational error — `decided: <message>`, exit 1
/// (NOT the exit-2 usage class). The not-a-directory check runs BEFORE the
/// config load, so a bad path wins exit 2 even beside a malformed config.
pub fn cmd_gate(args: &GateArgs) -> i32 {
    if !Path::new(&args.directory).is_dir() {
        return usage_error(&format!("not a directory: {}", args.directory));
    }
    if args.code && !args.full && args.base.is_none() {
        return usage_error("a diff base is required for --code unless --full is supplied");
    }
    let composed = match load_composed_or_exit(&args.directory, !args.top_level) {
        Ok(composed) => composed,
        Err(code) => return code,
    };
    let code_options = || {
        args.code.then_some(crate::gate::CodeGateOptions {
            repository: &args.repository,
            base: args.base.as_deref(),
            full_tree: args.full,
        })
    };
    let report = match if let Some(composed) = &composed {
        crate::gate::build_gate_with_composed(
            &args.directory,
            !args.top_level,
            code_options(),
            composed,
        )
    } else {
        crate::gate::build_gate_with_code(&args.directory, !args.top_level, code_options())
    } {
        Ok(report) => report,
        Err(exc) => {
            eprintln!("decided: {}", exc.message());
            return EXIT_VALIDATION_FAILED;
        }
    };
    if args.sarif {
        emit(output::render_gate_sarif(&report));
    } else if args.json {
        emit(output::render_gate_json(&report));
    } else {
        emit(output::render_gate_human(&report));
    }
    if report.ok() {
        EXIT_OK
    } else {
        EXIT_VALIDATION_FAILED
    }
}

// ---------------------------------------------------------------------------
// cmd_sentry
// ---------------------------------------------------------------------------

pub struct SentryArgs {
    pub directory: String,
    pub repository: String,
    pub base: Option<String>,
    pub full: bool,
    pub json: bool,
    pub sarif: bool,
    pub top_level: bool,
}

pub fn cmd_sentry(args: &SentryArgs) -> i32 {
    if !Path::new(&args.directory).is_dir() {
        return usage_error(&format!("not a directory: {}", args.directory));
    }
    let composed = match load_composed_or_exit(&args.directory, !args.top_level) {
        Ok(composed) => composed,
        Err(code) => return code,
    };
    let composed_items: Vec<_> = composed
        .as_ref()
        .map(|corpus| corpus.effective().cloned().collect())
        .unwrap_or_default();
    let report = match if let Some(composed) = &composed {
        crate::sentry::analyze_with_items_excluding(
            &args.directory,
            &args.repository,
            args.base.as_deref(),
            args.full,
            &composed_items,
            true,
            composed.read_only_roots(),
        )
    } else {
        crate::sentry::analyze(
            &args.directory,
            &args.repository,
            !args.top_level,
            args.base.as_deref(),
            args.full,
        )
    } {
        Ok(report) => report,
        Err(message) => return usage_error(&message),
    };
    if args.sarif {
        emit(output::render_sentry_sarif(&report));
    } else if args.json {
        emit(output::render_sentry_json(&report));
    } else {
        emit(output::render_sentry_human(&report));
    }
    if report.ok() {
        EXIT_OK
    } else {
        EXIT_VALIDATION_FAILED
    }
}

// ---------------------------------------------------------------------------
// cmd_herald
// ---------------------------------------------------------------------------

pub struct HeraldArgs {
    pub directory: String,
    pub paths_file: String,
    pub link_base: String,
    pub max_inline: i64,
    pub out: String,
    pub github_output: Option<String>,
    pub top_level: bool,
}

pub fn cmd_herald(args: &HeraldArgs) -> i32 {
    if !Path::new(&args.directory).is_dir() {
        return usage_error(&format!("not a directory: {}", args.directory));
    }
    if let Some(code) = refuse_read_only_targets(
        std::iter::once(args.out.as_str()).chain(args.github_output.as_deref()),
    ) {
        return code;
    }
    let paths = match std::fs::read_to_string(&args.paths_file) {
        Ok(text) => text.lines().map(str::trim).filter(|line| !line.is_empty()).map(str::to_string).collect::<Vec<_>>(),
        Err(error) => return usage_error(&format!("could not read paths file {}: {error}", args.paths_file)),
    };
    let report = match load_composed_or_exit(&args.directory, !args.top_level) {
        Ok(Some(composed)) => {
            crate::herald::collect_from_composed(&args.directory, &paths, &composed)
        }
        Ok(None) => crate::herald::collect(&args.directory, &paths, !args.top_level),
        Err(code) => return code,
    };
    let body = crate::herald::render(&report, &args.link_base, args.max_inline);
    if let Err(error) = std::fs::write(&args.out, body) {
        return usage_error(&format!("could not write Herald output {}: {error}", args.out));
    }
    let has_decisions = if report.has_decisions() { "true" } else { "false" };
    if let Some(path) = &args.github_output {
        use std::io::Write;
        let result = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .and_then(|mut file| writeln!(file, "has_decisions={has_decisions}"));
        if let Err(error) = result {
            return usage_error(&format!("could not write command output {path}: {error}"));
        }
    }
    emit(format!(
        "{} governing decision(s); has_decisions={has_decisions}",
        report.decisions.len()
    ));
    EXIT_OK
}

// ---------------------------------------------------------------------------
// cmd_watchkeeper
// ---------------------------------------------------------------------------

pub struct WatchkeeperArgs {
    pub directory: Option<String>,
    pub base: String,
    pub head: Option<String>,
    pub format: String, // human | json | github (choice-validated by the parser)
    pub json: bool,     // alias that OVERRIDES --format to json
    pub fail_on: String, // error | warning | none
    pub annotate: bool, // github format's stderr annotations (--no-annotate clears)
}

/// Review product knowledge changes between two repository states. Base and
/// head each name an existing directory (used as-is) or a git revision
/// materialized via `git archive`. Failure policy (v0.12.2): `error` fails
/// on a review recommendation, `warning` also on any warning-severity
/// finding, `none` never fails. Revision/repository errors are the exit-2
/// usage class (`decided: <msg>`).
pub fn cmd_watchkeeper(args: &WatchkeeperArgs) -> i32 {
    let directory = match &args.directory {
        Some(d) => d.clone(),
        // `decisions/` is the conventional knowledge root — compare it when
        // it exists; otherwise the current directory.
        None => {
            if Path::new("decisions").is_dir() {
                "decisions".to_string()
            } else {
                ".".to_string()
            }
        }
    };
    if !Path::new(&directory).is_dir() {
        return usage_error(&format!("not a directory: {directory}"));
    }
    let report = match crate::watchkeeper::build_watchkeeper_report(
        &directory,
        &args.base,
        args.head.as_deref(),
    ) {
        Ok(report) => report,
        Err(exc) => return usage_error(exc.message()),
    };
    let output_format = if args.json { "json" } else { args.format.as_str() };
    if output_format == "json" {
        emit(output::render_watchkeeper_json(&report));
    } else if output_format == "github" {
        // stdout is the step-summary Markdown; annotations go to stderr so
        // `> "$GITHUB_STEP_SUMMARY"` keeps them in the step log.
        emit(output::render_watchkeeper_github(&report));
        if args.annotate {
            for line in output::watchkeeper_annotations(&report) {
                eprintln!("{line}");
            }
        }
    } else {
        emit(output::render_watchkeeper_human(&report));
    }
    if args.fail_on == "none" {
        return EXIT_OK;
    }
    if report.review_recommended() {
        return EXIT_VALIDATION_FAILED;
    }
    if args.fail_on == "warning" && report.has_warnings() {
        return EXIT_VALIDATION_FAILED;
    }
    EXIT_OK
}

// ---------------------------------------------------------------------------
// cmd_doctor
// ---------------------------------------------------------------------------

pub struct DoctorArgs {
    pub directory: String,
    pub json: bool,
    pub top_level: bool,
    pub hub_threshold: i64,
}

/// Corpus health in one pass. Exits non-zero only on a validation or
/// relationship-integrity ERROR; orphan/hub/injection/unlinked/suspect
/// warnings exit 0 (REQ-007).
pub fn cmd_doctor(args: &DoctorArgs) -> i32 {
    if !Path::new(&args.directory).is_dir() {
        return usage_error(&format!("not a directory: {}", args.directory));
    }
    let recursive = !args.top_level;
    let report = match load_composed_or_exit(&args.directory, recursive) {
        Ok(Some(composed)) => crate::doctor::diagnose_composed(
            &args.directory,
            recursive,
            args.hub_threshold,
            &composed,
        ),
        Ok(None) => crate::doctor::diagnose(&args.directory, recursive, args.hub_threshold),
        Err(code) => return code,
    };
    if args.json {
        emit(output::render_doctor_json(&report));
    } else {
        emit(output::render_doctor_human(&report));
    }
    if report.ok() {
        EXIT_OK
    } else {
        EXIT_VALIDATION_FAILED
    }
}

// ---------------------------------------------------------------------------
// cmd_review
// ---------------------------------------------------------------------------

pub struct ReviewArgs {
    pub directory: String,
    pub json: bool,
    pub sarif: bool,
    pub top_level: bool,
    /// `--stale-after`: None when absent; Some(days) when present (const 14).
    pub stale_after: Option<i64>,
}

pub fn cmd_review(args: &ReviewArgs) -> i32 {
    if !Path::new(&args.directory).is_dir() {
        return usage_error(&format!("not a directory: {}", args.directory));
    }
    if let Some(days) = args.stale_after {
        if days < 0 {
            return usage_error("--stale-after must be a non-negative number of days");
        }
    }
    let recursive = !args.top_level;
    let report = match load_composed_or_exit(&args.directory, recursive) {
        Ok(Some(composed)) => crate::review::build_review_composed(
            &args.directory,
            recursive,
            args.stale_after,
            &composed,
        ),
        Ok(None) => crate::review::build_review(&args.directory, recursive, args.stale_after),
        Err(code) => return code,
    };
    if args.sarif {
        emit(output::render_review_sarif(&report));
    } else if args.json {
        emit(output::render_review_json(&report));
    } else {
        emit(output::render_review_human(&report));
    }
    if report.ok() {
        EXIT_OK
    } else {
        EXIT_VALIDATION_FAILED
    }
}

// ---------------------------------------------------------------------------
// cmd_export
// ---------------------------------------------------------------------------

pub struct ExportArgs {
    pub directory: String,
    pub json: bool,
    pub schema: Option<String>,
    pub graph: bool,
    pub documents: bool,
    pub html: bool,
    pub okf: bool,
    pub agent_rules: bool,
    pub check: bool,
    pub client: Vec<String>,
    pub out: Option<String>,
    /// Human diagnostic projection of the writable child layer only.
    pub local_only: bool,
}

struct HistoricalExportRevision {
    /// Owns the temporary snapshot through rendering and emission.
    _snapshot: crate::revisions::RevisionSnapshot,
    boundary: PathBuf,
    directory: String,
    corpus_existed: bool,
}

enum HistoricalExportError {
    Usage(String),
    Materialization(String),
}

impl From<crate::revisions::RevisionError> for HistoricalExportError {
    fn from(error: crate::revisions::RevisionError) -> Self {
        match error {
            crate::revisions::RevisionError::NotAGitRepository(message)
            | crate::revisions::RevisionError::RevisionNotFound(message) => Self::Usage(message),
        }
    }
}

impl From<crate::revisions::RevisionSnapshotError> for HistoricalExportError {
    fn from(error: crate::revisions::RevisionSnapshotError) -> Self {
        match error {
            crate::revisions::RevisionSnapshotError::NotAGitRepository(message)
            | crate::revisions::RevisionSnapshotError::RevisionNotFound(message) => {
                Self::Usage(message)
            }
            crate::revisions::RevisionSnapshotError::MaterializationFailed(message) => {
                Self::Materialization(message)
            }
        }
    }
}

fn lexical_absolute_path(directory: &str) -> Result<PathBuf, HistoricalExportError> {
    let input = Path::new(directory);
    let joined = if input.is_absolute() {
        input.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|error| {
                HistoricalExportError::Usage(format!(
                    "cannot resolve requested directory {directory}: {error}"
                ))
            })?
            .join(input)
    };
    let mut normalized = PathBuf::new();
    for component in joined.components() {
        match component {
            Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
            Component::RootDir => normalized.push(std::path::MAIN_SEPARATOR_STR),
            Component::CurDir => {}
            Component::ParentDir => {
                let _ = normalized.pop();
            }
            Component::Normal(value) => normalized.push(value),
        }
    }
    Ok(normalized)
}

fn lexical_git_discovery_directory(requested: &Path) -> Option<PathBuf> {
    let mut candidate = PathBuf::new();
    let mut nearest_directory = None;
    for component in requested.components() {
        candidate.push(component.as_os_str());
        match std::fs::symlink_metadata(&candidate) {
            Ok(metadata) if metadata.file_type().is_symlink() => break,
            Ok(metadata) if metadata.is_dir() => nearest_directory = Some(candidate.clone()),
            Ok(_) | Err(_) => break,
        }
    }
    nearest_directory
}

fn revision_repository_and_path(
    directory: &str,
) -> Result<(String, PathBuf, PathBuf), HistoricalExportError> {
    let requested = lexical_absolute_path(directory)?;
    let lexical_cwd = lexical_git_discovery_directory(&requested).ok_or_else(|| {
        HistoricalExportError::Usage(format!("not a git repository: {directory}"))
    })?;
    let (repository_root, git_cwd) =
        match crate::revisions::repository_root(&lexical_cwd.to_string_lossy()) {
            Ok(repository_root) => (repository_root, lexical_cwd),
            Err(_) => {
                // A checkout itself may be addressed through a symlink. No
                // lexical Git ancestor owns that path, so physical discovery
                // is safe; an in-repository corpus symlink never reaches this
                // fallback because its lexical repository wins above.
                let mut physical_cwd = requested.clone();
                while !physical_cwd.is_dir() {
                    if !physical_cwd.pop() {
                        return Err(HistoricalExportError::Usage(format!(
                            "not a git repository: {directory}"
                        )));
                    }
                }
                let repository_root =
                    crate::revisions::repository_root(&physical_cwd.to_string_lossy())?;
                (repository_root, physical_cwd)
            }
        };
    let repository_path = std::fs::canonicalize(&repository_root).map_err(|error| {
        HistoricalExportError::Usage(format!(
            "cannot resolve repository root {repository_root}: {error}"
        ))
    })?;
    let relative = if let Ok(relative) = requested.strip_prefix(&repository_path) {
        relative.to_path_buf()
    } else {
        let physical_cwd = std::fs::canonicalize(&git_cwd).map_err(|error| {
            HistoricalExportError::Usage(format!(
                "cannot resolve Git discovery directory {}: {error}",
                git_cwd.display()
            ))
        })?;
        let physical_prefix = physical_cwd.strip_prefix(&repository_path).map_err(|_| {
            HistoricalExportError::Usage(format!(
                "requested directory is outside Git repository: {directory}"
            ))
        })?;
        let unresolved = requested.strip_prefix(&git_cwd).map_err(|_| {
            HistoricalExportError::Usage(format!(
                "cannot derive repository-relative path for {directory}"
            ))
        })?;
        physical_prefix.join(unresolved)
    };
    Ok((repository_root, repository_path, relative))
}

fn node_path(node: &Path, relative: &str) -> PathBuf {
    if node.as_os_str().is_empty() {
        PathBuf::from(relative)
    } else {
        node.join(relative)
    }
}

fn materialize_node_metadata(
    snapshot: &mut crate::revisions::RevisionSnapshot,
    node: &Path,
) -> Result<(), HistoricalExportError> {
    for relative in [
        crate::federation::CONFIG_RELATIVE_PATH,
        crate::federation::MANIFEST_RELATIVE_PATH,
    ] {
        snapshot
            .materialize_path(
                node_path(node, relative),
                crate::revisions::MissingPathPolicy::Ignore,
            )
            .map_err(HistoricalExportError::from)?;
    }
    Ok(())
}

fn materialize_ancestor_metadata(
    snapshot: &mut crate::revisions::RevisionSnapshot,
    corpus: &Path,
) -> Result<(), HistoricalExportError> {
    let mut ancestor = corpus.to_path_buf();
    loop {
        materialize_node_metadata(snapshot, &ancestor)?;
        if !ancestor.pop() {
            return Ok(());
        }
    }
}

fn historical_composition_root(snapshot_root: &Path, corpus: &Path) -> PathBuf {
    for ancestor in corpus.ancestors() {
        if !ancestor.starts_with(snapshot_root) {
            break;
        }
        let configured = ancestor
            .join(crate::federation::CONFIG_RELATIVE_PATH)
            .is_file();
        let manifested = ancestor
            .join(crate::federation::MANIFEST_RELATIVE_PATH)
            .is_file();
        if configured || manifested {
            return ancestor
                .strip_prefix(snapshot_root)
                .unwrap_or_else(|_| Path::new(""))
                .to_path_buf();
        }
    }
    corpus
        .strip_prefix(snapshot_root)
        .unwrap_or_else(|_| Path::new(""))
        .to_path_buf()
}

fn has_historical_governing_config(snapshot_root: &Path, corpus: &Path) -> bool {
    corpus.ancestors().any(|ancestor| {
        ancestor.starts_with(snapshot_root)
            && ancestor
                .join(crate::federation::CONFIG_RELATIVE_PATH)
                .is_file()
    })
}

fn historical_parent_declarations(
    node: &Path,
) -> Option<(Vec<crate::federation::ParentDeclaration>, bool)> {
    match crate::federation::load_graph_manifest(node) {
        Ok(Some(manifest)) => Some((manifest.parents, true)),
        Ok(None) => match crate::federation::load_manifest(node) {
            Ok(Some(manifest)) => Some((vec![manifest.inherits], false)),
            Ok(None) | Err(_) => None,
        },
        Err(_) => None,
    }
}

fn historical_parent_root_exclusions(
    snapshot_root: &Path,
    node: &Path,
    corpus: &Path,
) -> Vec<PathBuf> {
    let absolute = snapshot_root.join(node);
    let Some((parents, _)) = historical_parent_declarations(&absolute) else {
        return Vec::new();
    };
    let mut exclusions: Vec<PathBuf> = parents
        .into_iter()
        .map(|parent| node_path(node, &parent.root))
        .filter(|parent_root| parent_root.starts_with(corpus))
        .collect();
    exclusions.sort();
    exclusions.dedup();
    exclusions
}

fn historical_corpus_symlink_policy(
    snapshot_root: &Path,
    node: &Path,
) -> crate::revisions::CorpusSymlinkPolicy {
    match historical_parent_declarations(&snapshot_root.join(node)) {
        Some((_, true)) => crate::revisions::CorpusSymlinkPolicy::RejectAll,
        Some((_, false)) | None => crate::revisions::CorpusSymlinkPolicy::CorpusFilesOnly,
    }
}

fn materialize_federation_closure(
    snapshot: &mut crate::revisions::RevisionSnapshot,
    root: PathBuf,
) -> Result<(), HistoricalExportError> {
    let mut queue = VecDeque::from([(root.clone(), 0usize)]);
    let mut visited = BTreeSet::new();
    let mut materialized_nodes = BTreeSet::from([root]);
    let mut materialized_corpora = BTreeSet::new();
    let mut edges = 0usize;

    while let Some((node, depth)) = queue.pop_front() {
        if !visited.insert(node.clone()) {
            continue;
        }
        if depth > crate::federation::V2_MAX_INHERITANCE_DEPTH {
            return Err(HistoricalExportError::Materialization(format!(
                "historical federation exceeds maximum inheritance depth {}",
                crate::federation::V2_MAX_INHERITANCE_DEPTH
            )));
        }
        let absolute = snapshot.root().join(&node);
        let Some((parents, recursive)) = historical_parent_declarations(&absolute) else {
            // A malformed manifest is deliberately left for the established
            // federation loader, which owns its stable validation diagnostic.
            continue;
        };
        edges = edges.saturating_add(parents.len());
        if edges > crate::federation::V2_MAX_EDGES {
            return Err(HistoricalExportError::Materialization(format!(
                "historical federation exceeds maximum edge count {}",
                crate::federation::V2_MAX_EDGES
            )));
        }
        for parent in parents {
            let parent_root = node_path(&node, &parent.root);
            if materialized_nodes.insert(parent_root.clone()) {
                materialize_node_metadata(snapshot, &parent_root)?;
            }
            let parent_corpus = node_path(&parent_root, &parent.corpus);
            if materialized_corpora.insert(parent_corpus.clone()) {
                let exclusions = historical_parent_root_exclusions(
                    snapshot.root(),
                    &parent_root,
                    &parent_corpus,
                );
                snapshot
                    .materialize_corpus_with_options(
                        parent_corpus,
                        crate::revisions::MissingPathPolicy::Error,
                        &exclusions,
                        if recursive {
                            crate::revisions::CorpusSymlinkPolicy::RejectAll
                        } else {
                            crate::revisions::CorpusSymlinkPolicy::CorpusFilesOnly
                        },
                    )
                    .map_err(HistoricalExportError::from)?;
            }
            if recursive {
                queue.push_back((parent_root, depth + 1));
            }
        }
    }
    Ok(())
}

fn materialize_export_revision(
    directory: &str,
    revision: &str,
) -> Result<HistoricalExportRevision, HistoricalExportError> {
    let (repository_root, repository_path, relative) =
        revision_repository_and_path(directory)?;
    let mut snapshot = crate::revisions::RevisionSnapshot::open(&repository_root, revision)?;
    materialize_ancestor_metadata(&mut snapshot, &relative)?;
    let projected_corpus = snapshot.root().join(&relative);
    let composition_root = historical_composition_root(snapshot.root(), &projected_corpus);
    let exclusions = historical_parent_root_exclusions(
        snapshot.root(),
        &composition_root,
        &relative,
    );
    let symlink_policy =
        historical_corpus_symlink_policy(snapshot.root(), &composition_root);
    let corpus = snapshot
        .materialize_corpus_with_options(
            &relative,
            crate::revisions::MissingPathPolicy::EmptyDirectory,
            &exclusions,
            symlink_policy,
        )
        .map_err(HistoricalExportError::from)?;
    if !has_historical_governing_config(snapshot.root(), &corpus.path) {
        if let Some(config) = crate::validate::find_config_file(directory) {
            let config = std::fs::canonicalize(&config).map_err(|error| {
                HistoricalExportError::Usage(format!(
                    "cannot resolve governing config {}: {error}",
                    config.display()
                ))
            })?;
            if !config.starts_with(&repository_path) {
                return Err(HistoricalExportError::Usage(format!(
                    "governing config {} is outside Git repository {} and cannot be reproduced by --at",
                    config.display(),
                    repository_path.display()
                )));
            }
        }
    }
    if corpus.existed {
        materialize_federation_closure(&mut snapshot, composition_root)?;
    }
    let boundary = snapshot.root().to_path_buf();
    let directory = corpus.path.to_string_lossy().into_owned();
    Ok(HistoricalExportRevision {
        _snapshot: snapshot,
        boundary,
        directory,
        corpus_existed: corpus.existed,
    })
}

pub fn cmd_export(args: &ExportArgs) -> i32 {
    cmd_export_at(args, None)
}

/// CLI-only revision projection, kept separate so adding `--at` does not
/// change the released public [`ExportArgs`] construction contract.
pub(crate) fn cmd_export_at(args: &ExportArgs, at: Option<&str>) -> i32 {
    if args.schema.is_none() && at.is_none() && !Path::new(&args.directory).is_dir() {
        return usage_error(&format!("not a directory: {}", args.directory));
    }
    if args.local_only && (args.okf || args.agent_rules || args.schema.is_some()) {
        return usage_error(
            "--local-only is available only for viewer, documents, and graph exports",
        );
    }
    if at.is_some() && (args.html || args.okf || args.agent_rules || args.schema.is_some()) {
        return usage_error("--at is available only for viewer, documents, and graph exports");
    }
    // Agent-rules is a distinct mode (ADR-067) owning --out/--client/--check
    // and --json; it dispatches before the export-payload guards.
    if args.agent_rules {
        return cmd_agent_rules(args);
    }
    if args.check {
        return usage_error("--check requires --agent-rules");
    }
    if !args.client.is_empty() {
        return usage_error("--client requires --agent-rules");
    }
    if args.json && (args.html || args.okf) {
        return usage_error("--json cannot combine with --html or --okf");
    }
    if args.out.is_some() && !(args.html || args.okf) {
        return usage_error("--out requires --html or --okf (--json writes to stdout)");
    }
    // Classify explicit/default write targets before loading corpus content.
    // A version-2 root deliberately does not enter the version-1 composition
    // loader, but its inherited materialisations remain read-only even while
    // graph command wiring is staged separately.
    if args.html || args.okf {
        let out = args.out.as_deref().unwrap_or(if args.okf {
            "okf-bundle"
        } else {
            "lore-export.html"
        });
        if let Some(code) = refuse_read_only_target(out) {
            return code;
        }
    }
    if let Some(name) = &args.schema {
        let Some(schema) = crate::export::export_schema(name) else {
            return usage_error(&format!(
                "unknown export schema: {name} (choose from viewer, documents, graph)"
            ));
        };
        emit_exact(schema);
        return EXIT_OK;
    }
    let historical = match at {
        Some(revision) => match materialize_export_revision(&args.directory, revision) {
            Ok(snapshot) => Some(snapshot),
            Err(HistoricalExportError::Usage(message)) => return usage_error(&message),
            Err(HistoricalExportError::Materialization(message)) => {
                eprintln!("decided: {message}");
                return EXIT_VALIDATION_FAILED;
            }
        },
        None => None,
    };
    let export_directory = historical
        .as_ref()
        .map_or(args.directory.as_str(), |snapshot| snapshot.directory.as_str());
    let identity_directory = args.directory.as_str();
    let snapshot_boundary = historical
        .as_ref()
        .map(|snapshot| snapshot.boundary.as_path());
    let historical_corpus_absent = historical
        .as_ref()
        .is_some_and(|snapshot| !snapshot.corpus_existed);
    let composed = if historical_corpus_absent {
        None
    } else {
        match load_composed_or_exit_with_boundary(export_directory, true, snapshot_boundary) {
            Ok(corpus) => corpus,
            Err(code) => return code,
        }
    };
    if args.documents {
        let export = match composed.as_ref() {
            Some(corpus) => crate::export::build_documents_export_from_composed_for(
                export_directory,
                identity_directory,
                corpus,
                args.local_only,
                snapshot_boundary,
            )
            .map_err(|error| error.message().to_string()),
            None => crate::export::build_documents_export_for(
                export_directory,
                identity_directory,
                snapshot_boundary,
            )
            .map_err(|error| error.message().to_string()),
        };
        let export = match export {
            Ok(export) => export,
            Err(message) => {
                eprintln!("decided: {message}");
                return EXIT_VALIDATION_FAILED;
            }
        };
        let rendered = output::render_documents_jsonl(&export);
        if historical_corpus_absent && rendered.is_empty() {
            emit_exact("");
        } else {
            emit(rendered);
        }
        return EXIT_OK;
    }
    if args.graph {
        let export = match composed.as_ref() {
            Some(corpus) => crate::export::build_graph_export_from_composed_for(
                export_directory,
                identity_directory,
                corpus,
                args.local_only,
                snapshot_boundary,
            )
            .map_err(|error| error.message().to_string()),
            None => crate::export::build_graph_export_for(
                export_directory,
                identity_directory,
                snapshot_boundary,
            )
            .map_err(|error| error.message().to_string()),
        };
        let export = match export {
            Ok(export) => export,
            Err(message) => {
                eprintln!("decided: {message}");
                return EXIT_VALIDATION_FAILED;
            }
        };
        emit(output::render_graph_json(&export));
        return EXIT_OK;
    }
    // OKF consumes source Markdown directly, so its projection skips the
    // unrelated HTML rendering used by the viewer export.
    if args.okf {
        let export = match composed.as_ref() {
            Some(corpus) => crate::export::build_okf_export_from_composed(
                &args.directory,
                output::rac_version(),
                corpus,
            ),
            None => crate::export::build_okf_export(&args.directory, output::rac_version()),
        };
        let out = args.out.as_deref().unwrap_or("okf-bundle");
        let recency = crate::okf::artifact_recency(&args.directory, &export);
        let bundle = match crate::okf::render_okf_bundle(&export, &recency, &args.directory) {
            Ok(bundle) => bundle,
            Err(msg) => {
                // The oracle's uncaught ValueError: a Python traceback on
                // stderr, exit 1, nothing written. Stderr bytes are a
                // documented divergence; the exit code and no-write
                // behavior are the contract.
                eprintln!("ValueError: {msg}");
                return EXIT_VALIDATION_FAILED;
            }
        };
        let destinations: Vec<_> = bundle
            .keys()
            .map(|relative| std::path::Path::new(out).join(relative))
            .collect();
        let destination_text: Vec<_> = destinations
            .iter()
            .map(|path| path.to_string_lossy().into_owned())
            .collect();
        if let Some(code) =
            refuse_read_only_targets(destination_text.iter().map(String::as_str))
        {
            return code;
        }
        for (rel, content) in &bundle {
            let dest = std::path::Path::new(out).join(rel);
            let written = dest
                .parent()
                .map(std::fs::create_dir_all)
                .unwrap_or(Ok(()))
                .and_then(|_| std::fs::write(&dest, content));
            if let Err(exc) = written {
                return usage_error(&format!("cannot write {out}: {exc}"));
            }
        }
        let edges = export.relationships.len();
        emit(format!(
            "wrote {out}/ \u{2014} {} artifact(s), {edges} relationship(s)",
            export.artifact_count()
        ));
        return EXIT_OK;
    }
    let export = match composed.as_ref() {
        Some(corpus) => crate::export::build_corpus_export_from_composed_for(
            export_directory,
            identity_directory,
            output::rac_version(),
            corpus,
            args.local_only,
            snapshot_boundary,
        )
        .map_err(|error| error.message().to_string()),
        None => crate::export::build_corpus_export_for(
            export_directory,
            identity_directory,
            output::rac_version(),
            snapshot_boundary,
        )
            .map_err(|error| error.message().to_string()),
    };
    let export = match export {
        Ok(export) => export,
        Err(message) => {
            eprintln!("decided: {message}");
            return EXIT_VALIDATION_FAILED;
        }
    };

    // JSON is the default mode: the payload is the product (--json a no-op).
    if !args.html {
        emit(output::render_export_json(&export));
        return EXIT_OK;
    }

    let out = args.out.as_deref().unwrap_or("lore-export.html");
    let html = match if composed.is_some() {
        crate::portal::render_federated_export_html(&export)
    } else {
        crate::portal::render_export_html(&export)
    } {
        Ok(html) => html,
        Err(msg) => return usage_error(&msg), // PortalSeamMissing (unreachable)
    };
    // Path(out).write_text: no parent mkdir — a missing directory is the
    // OSError path (exit 2).
    if let Err(exc) = std::fs::write(out, html) {
        return usage_error(&format!("cannot write {out}: {exc}"));
    }
    let edges = export.relationships.len();
    emit(format!(
        "wrote {out} \u{2014} {} artifact(s), {edges} relationship(s)",
        export.artifact_count()
    ));
    EXIT_OK
}

/// `_cmd_agent_rules(args)` — `decided export --agent-rules [--check]`
/// (v0.21.15, ADR-067). `--check` never writes and exits 1 on drift.
fn cmd_agent_rules(args: &ExportArgs) -> i32 {
    // Invalid --client values were already rejected by the argv parser
    // (argparse choices), so `unknown_clients` is unreachable here.
    let root = crate::agent_rules::agent_rules_root(&args.directory, args.out.as_deref());
    if let Some(code) = refuse_read_only_target(&root) {
        return code;
    }
    if !args.check {
        let targets = crate::agent_rules::output_targets(&root, &args.client);
        if let Some(code) = refuse_read_only_targets(targets.iter().map(String::as_str)) {
            return code;
        }
    }
    let result = if args.check {
        match crate::agent_rules::check_agent_rules(&args.directory, &root, &args.client) {
            Ok(result) => result,
            Err(exc) => return usage_error(&format!("cannot read corpus: {exc}")),
        }
    } else {
        match crate::agent_rules::generate_agent_rules(&args.directory, &root, &args.client) {
            Ok(result) => result,
            Err(exc) => return usage_error(&format!("cannot write under {root}: {exc}")),
        }
    };

    if args.json {
        emit(output::render_agent_rules_json(&result));
    } else {
        emit(output::render_agent_rules_human(&result));
    }

    if args.check && result.drifted() {
        return EXIT_VALIDATION_FAILED;
    }
    EXIT_OK
}

// ---------------------------------------------------------------------------
// cmd_schema / cmd_templates
// ---------------------------------------------------------------------------

pub struct SchemaArgs {
    pub schema: Option<String>,
    pub list: bool,
    pub json: bool,
    pub template: bool,
}

pub fn cmd_schema(args: &SchemaArgs) -> i32 {
    let names = crate::spec::available_schemas();
    if args.list {
        if args.template {
            return usage_error("--template cannot be used with --list");
        }
        if args.schema.is_some() {
            return usage_error("schema name cannot be used with --list");
        }
        if args.json {
            emit(output::render_schema_list_json(&names));
        } else {
            emit(output::render_schema_list_human(&names));
        }
        return EXIT_OK;
    }

    let Some(name) = &args.schema else {
        return usage_error("schema name required unless --list is passed");
    };

    let Some(spec) = crate::spec::spec_for(name) else {
        // Unknown schema: multi-line blob to stderr, exit 2 (no `decided:` prefix).
        eprintln!("{}", output::render_unknown_schema(name, &names));
        return EXIT_USAGE;
    };

    if args.json {
        emit(output::render_schema_json(spec));
    } else if args.template {
        emit(output::render_schema_template(spec));
    } else {
        emit(output::render_schema_human(spec));
    }
    EXIT_OK
}

pub struct TemplatesArgs {
    pub json: bool,
}

pub fn cmd_templates(args: &TemplatesArgs) -> i32 {
    let names = crate::spec::available_schemas();
    if args.json {
        emit(output::render_templates_json(&names));
    } else {
        emit(output::render_templates_human(&names));
    }
    EXIT_OK
}

// ---------------------------------------------------------------------------
// cmd_resolve / cmd_find (PORT-CONTRACT.d/06)
// ---------------------------------------------------------------------------

pub struct ResolveArgs {
    pub id: String,
    pub directory: String,
    pub json: bool,
    pub top_level: bool,
}

fn composed_resolution(
    corpus: &crate::composition::ComposedCorpus,
    artifact_id: &str,
) -> Result<crate::resolve::ResolutionResult, crate::composition::LookupError> {
    let reference = crate::pycompat::py_strip(artifact_id);
    match corpus.resolve(reference) {
        Ok(item) => {
            let entry = crate::resolve::identity_entry_from_item(item);
            Ok(crate::resolve::ResolutionResult {
                artifact_id: artifact_id.to_string(),
                outcome: crate::resolve::OUTCOME_RESOLVED,
                artifact: Some(crate::resolve::resolved_from_entry(&entry)),
                duplicate_paths: Vec::new(),
            })
        }
        Err(crate::composition::LookupError::NotFound) => Ok(crate::resolve::ResolutionResult {
            artifact_id: artifact_id.to_string(),
            outcome: crate::resolve::OUTCOME_NOT_FOUND,
            artifact: None,
            duplicate_paths: Vec::new(),
        }),
        Err(crate::composition::LookupError::Ambiguous(keys)) => {
            let mut paths: Vec<String> = keys
                .iter()
                .filter_map(|key| corpus.item(key))
                .map(|item| {
                    format!(
                        "{}::{}",
                        item.artifact_path.source, item.artifact_path.relative_path
                    )
                })
                .collect();
            paths.sort();
            Ok(crate::resolve::ResolutionResult {
                artifact_id: artifact_id.to_string(),
                outcome: if corpus.is_graph() {
                    crate::resolve::OUTCOME_AMBIGUOUS
                } else {
                    crate::resolve::OUTCOME_DUPLICATE
                },
                artifact: None,
                duplicate_paths: paths,
            })
        }
        Err(error) => Err(error),
    }
}

pub fn cmd_resolve(args: &ResolveArgs) -> i32 {
    if !Path::new(&args.directory).is_dir() {
        return usage_error(&format!("not a directory: {}", args.directory));
    }
    let composed = match load_composed_or_exit(&args.directory, !args.top_level) {
        Ok(composed) => composed,
        Err(code) => return code,
    };
    let result = if let Some(composed) = &composed {
        match composed_resolution(composed, &args.id) {
            Ok(result) => result,
            Err(crate::composition::LookupError::QualifiedCanonicalRequired) => {
                eprintln!(
                    "decided: qualified references require a canonical artifact ID after `::`: {}",
                    args.id
                );
                return EXIT_VALIDATION_FAILED;
            }
            Err(_) => {
                eprintln!("decided: invalid qualified artifact reference: {}", args.id);
                return EXIT_VALIDATION_FAILED;
            }
        }
    } else {
        crate::resolve::resolve_artifact(&args.directory, &args.id, !args.top_level)
    };
    if args.json {
        emit(if let Some(composed) = &composed {
            output::render_resolve_json_with_composed(&result, composed)
        } else {
            output::render_resolve_json(&result)
        });
    } else if result.outcome == crate::resolve::OUTCOME_RESOLVED {
        let artifact = result.artifact.as_ref().expect("resolved implies artifact");
        emit(if composed.is_some() {
            output::render_resolve_human_with_origin(artifact)
        } else {
            output::render_resolve_human(artifact)
        });
    } else if result.outcome == crate::resolve::OUTCOME_DUPLICATE
        || result.outcome == crate::resolve::OUTCOME_AMBIGUOUS
    {
        let found: Vec<String> = result
            .duplicate_paths
            .iter()
            .map(|p| format!("- {p}"))
            .collect();
        let label = if result.outcome == crate::resolve::OUTCOME_AMBIGUOUS {
            "ambiguous artifact ID"
        } else {
            "duplicate artifact ID"
        };
        eprintln!("decided: {label}: {}\n\nFound in:\n{}", args.id, found.join("\n"));
    } else {
        eprintln!("decided: artifact not found: {}", args.id);
    }
    // Not-found and duplicate identity are both repository findings (exit 1).
    if result.outcome == crate::resolve::OUTCOME_RESOLVED {
        EXIT_OK
    } else {
        EXIT_VALIDATION_FAILED
    }
}

pub struct FindArgs {
    pub query: String,
    pub directory: String,
    pub artifact_type: Option<String>,
    pub decisions: bool,
    pub tags: Vec<String>,
    pub json: bool,
    pub explain: bool,
    pub top_level: bool,
    /// The live-only facet (ADR-113): drop retired matches of every type.
    pub live: bool,
    /// `--cache` / `--no-cache` (ADR-112: on by default).
    pub cache: bool,
    /// `--verify`: force the full-hash freshness floor on the cache path.
    pub verify: bool,
}

/// `annotate_search_recency(matches, directory)` — the read-surface join
/// (ADR-045): git-derived staleness per match, computed AFTER ranking so the
/// matched set and order are unchanged. All-null outside a git repository.
/// Shared by `cmd_find` and the MCP `search_artifacts` tool (both surfaces
/// are byte-identical on this join).
pub fn annotate_search_recency(matches: &mut [crate::resolve::ResolvedArtifact], directory: &str) {
    use crate::gitinfo;
    if matches.is_empty() {
        return;
    }
    let timing_started = crate::timing::start();
    let threshold = crate::validate::load_freshness_threshold(directory);
    let reference = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let repo_root = gitinfo::repository_root(Path::new(directory));
    let paths: Vec<PathBuf> = matches.iter().map(|m| PathBuf::from(&m.path)).collect();
    let committed = match &repo_root {
        Some(root) => gitinfo::last_committed_for_paths_in_repo(root, &paths),
        None => paths.into_iter().map(|path| (path, None)).collect(),
    };
    for (m, (_, last)) in matches.iter_mut().zip(committed) {
        let st = gitinfo::staleness(last.as_deref(), threshold, reference);
        m.recency = Some(crate::resolve::Recency {
            last_committed: st
                .last_committed
                .as_deref()
                .map(gitinfo::isoformat_roundtrip),
            age_days: st.age_days,
            stale: st.stale,
        });
    }
    crate::timing::emit_since(
        "git.recency_join",
        timing_started,
        &[
            ("matches", matches.len() as u64),
            ("repository", u64::from(repo_root.is_some())),
        ],
    );
}

/// Join Git recency onto local matches from a composed corpus without ever
/// attributing the child checkout's history to inherited artifacts.
///
/// Ranking has already completed when this runs. Inherited records retain a
/// missing `recency` field; local records use their runtime-only physical
/// locator while public paths remain owning-source-relative.
pub fn annotate_composed_search_recency(
    matches: &mut [crate::resolve::ResolvedArtifact],
    directory: &str,
    corpus: &crate::composition::ComposedCorpus,
) {
    use crate::corpus::Layer;
    use crate::gitinfo;

    let local: Vec<(usize, PathBuf)> = matches
        .iter()
        .enumerate()
        .filter_map(|(index, artifact)| {
            let key = artifact.key.as_ref()?;
            let item = corpus.item(key)?;
            (item.origin.layer == Layer::Local)
                .then(|| (index, item.locator.path.clone()))
        })
        .collect();
    if local.is_empty() {
        return;
    }
    let local_count = local.len();

    let timing_started = crate::timing::start();
    let threshold = crate::validate::load_freshness_threshold(directory);
    let reference = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0);
    let repo_root = gitinfo::repository_root(Path::new(directory));
    let paths: Vec<PathBuf> = local.iter().map(|(_, path)| path.clone()).collect();
    let committed = match &repo_root {
        Some(root) => gitinfo::last_committed_for_paths_in_repo(root, &paths),
        None => paths.into_iter().map(|path| (path, None)).collect(),
    };
    for ((index, _), (_, last)) in local.into_iter().zip(committed) {
        let staleness = gitinfo::staleness(last.as_deref(), threshold, reference);
        matches[index].recency = Some(crate::resolve::Recency {
            last_committed: staleness
                .last_committed
                .as_deref()
                .map(gitinfo::isoformat_roundtrip),
            age_days: staleness.age_days,
            stale: staleness.stale,
        });
    }
    crate::timing::emit_since(
        "git.recency_join",
        timing_started,
        &[
            ("matches", matches.len() as u64),
            ("local_matches", local_count as u64),
            ("repository", u64::from(repo_root.is_some())),
        ],
    );
}

/// Graph-v2 equivalent of [`annotate_composed_search_recency`]. Only records
/// owned by the root source may use the root repository's Git history;
/// inherited matches deliberately retain absent recency.
pub fn annotate_graph_search_recency(
    matches: &mut [crate::resolve::ResolvedArtifact],
    directory: &str,
    corpus: &crate::graph_federated_corpus::VerifiedGraphCorpus,
) {
    use crate::corpus::Layer;
    use crate::gitinfo;

    let local: Vec<(usize, PathBuf)> = matches
        .iter()
        .enumerate()
        .filter_map(|(index, artifact)| {
            let key = artifact.key.as_ref()?;
            let item = corpus.composition.item(key)?;
            (item.origin.layer == Layer::Local).then(|| (index, item.locator.path.clone()))
        })
        .collect();
    if local.is_empty() {
        return;
    }
    let local_count = local.len();
    let timing_started = crate::timing::start();
    let threshold = crate::validate::load_freshness_threshold(directory);
    let reference = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0);
    let repo_root = gitinfo::repository_root(Path::new(directory));
    let paths: Vec<PathBuf> = local.iter().map(|(_, path)| path.clone()).collect();
    let committed = match &repo_root {
        Some(root) => gitinfo::last_committed_for_paths_in_repo(root, &paths),
        None => paths.into_iter().map(|path| (path, None)).collect(),
    };
    for ((index, _), (_, last)) in local.into_iter().zip(committed) {
        let staleness = gitinfo::staleness(last.as_deref(), threshold, reference);
        matches[index].recency = Some(crate::resolve::Recency {
            last_committed: staleness
                .last_committed
                .as_deref()
                .map(gitinfo::isoformat_roundtrip),
            age_days: staleness.age_days,
            stale: staleness.stale,
        });
    }
    crate::timing::emit_since(
        "git.recency_join",
        timing_started,
        &[
            ("matches", matches.len() as u64),
            ("local_matches", local_count as u64),
            ("repository", u64::from(repo_root.is_some())),
        ],
    );
}

/// Serve `decided find` from the persistent index store (`_find_from_store`,
/// ADR-112): a warm run against an unchanged corpus reads the mapped base;
/// a cold run builds fresh, writes the store, and serves either the
/// reopened view or the fresh structures (ADR-080).
fn find_from_store(args: &FindArgs) -> crate::resolve::SearchResult {
    use crate::derived_cache::{DerivedIndexCache, ReadModel};
    let view = DerivedIndexCache::default().load_or_build(
        &args.directory,
        !args.top_level,
        args.verify,
    );
    match view {
        ReadModel::View(reader) => {
            if args.decisions {
                crate::read_model::store_find_decisions(&reader, &args.query)
            } else {
                crate::read_model::store_search(
                    &reader,
                    &args.query,
                    args.artifact_type.as_deref(),
                    &args.tags,
                    args.live,
                )
            }
        }
        ReadModel::Fresh(derived) => {
            if args.decisions {
                crate::read_model::find_decisions_in(
                    &derived.index_entries,
                    &derived.live_decision_paths,
                    &args.query,
                )
            } else {
                crate::resolve::search_index_filtered(
                    &derived.index_entries,
                    &args.query,
                    args.artifact_type.as_deref(),
                    &args.tags,
                    args.live,
                )
            }
        }
    }
}

fn cmd_find_graph(args: &FindArgs, repository_root: &Path, corpus_relative: &str) -> i32 {
    use crate::derived_cache::{GraphFederatedCacheTracker, ReadModel};

    let persistent_cache = crate::derived_cache::cache_enabled(args.cache);
    let mut tracker = GraphFederatedCacheTracker::new(
        crate::derived_cache::default_cache_dir(),
    );
    let read = match tracker.read_graph(
        repository_root,
        corpus_relative,
        !args.top_level,
        persistent_cache,
    ) {
        Ok(read) => read,
        Err(error) => {
            eprintln!("decided: {error}");
            return EXIT_VALIDATION_FAILED;
        }
    };
    let mut result = match read.model {
        ReadModel::View(reader) => {
            if args.decisions {
                crate::read_model::store_find_decisions(reader, &args.query)
            } else {
                crate::read_model::store_search(
                    reader,
                    &args.query,
                    args.artifact_type.as_deref(),
                    &args.tags,
                    args.live,
                )
            }
        }
        ReadModel::Fresh(derived) => {
            if args.decisions {
                crate::read_model::find_decisions_in_source_aware(
                    &derived.index_entries,
                    &derived.live_decision_keys,
                    &derived.live_decision_paths,
                    &args.query,
                )
            } else {
                crate::resolve::search_index_filtered(
                    &derived.index_entries,
                    &args.query,
                    args.artifact_type.as_deref(),
                    &args.tags,
                    args.live,
                )
            }
        }
    };
    annotate_graph_search_recency(&mut result.matches, &args.directory, read.corpus);
    let render_started = crate::timing::start();
    let rendered = if args.json {
        output::render_find_json_with_graph(&result, args.explain, read.corpus)
    } else {
        output::render_find_human_with_graph(&result, args.explain, read.corpus)
    };
    crate::timing::emit_since(
        "cli.response_serialize",
        render_started,
        &[("matches", result.matches.len() as u64), ("bytes", rendered.len() as u64)],
    );
    emit(rendered);
    EXIT_OK
}

pub fn cmd_find(args: &FindArgs) -> i32 {
    if !Path::new(&args.directory).is_dir() {
        return usage_error(&format!("not a directory: {}", args.directory));
    }
    let graph = match crate::federated_corpus::graph_cache_location(&args.directory) {
        Ok(graph) => graph,
        Err(error) => {
            eprintln!("decided: {error}");
            return EXIT_VALIDATION_FAILED;
        }
    };
    if let Some((repository_root, corpus_relative)) = graph {
        return cmd_find_graph(args, &repository_root, &corpus_relative);
    }
    let composed = match load_composed_or_exit(&args.directory, !args.top_level) {
        Ok(composed) => composed,
        Err(code) => return code,
    };
    let mut result = if let Some(composed) = &composed {
        let entries = composed.effective_index();
        if args.decisions {
            let live_keys: Vec<crate::corpus::ArtifactKey> = composed
                .effective()
                .filter(|item| {
                    item.spec.map(|spec| spec.name.as_str()) == Some("decision")
                        && crate::resolve::is_live_decision(&item.artifact)
                })
                .map(|item| item.key.clone())
                .collect();
            crate::read_model::find_decisions_in_source_aware(
                &entries,
                &live_keys,
                &[],
                &args.query,
            )
        } else {
            crate::resolve::search_index_filtered(
                &entries,
                &args.query,
                args.artifact_type.as_deref(),
                &args.tags,
                args.live,
            )
        }
    } else if crate::derived_cache::cache_enabled(args.cache) {
        // Default store reuse (ADR-112): serve from the persistent index
        // store instead of a fresh walk, byte-identical to the walk below.
        find_from_store(args)
    } else if args.decisions {
        // The live decision query (ADR-067): decision type filter + the
        // Accepted/non-retired liveness filter; `--tag` is silently ignored.
        crate::resolve::find_decisions(&args.directory, &args.query, !args.top_level)
    } else {
        crate::resolve::find_artifacts(
            &args.directory,
            &args.query,
            args.artifact_type.as_deref(),
            !args.top_level,
            &args.tags,
            args.live,
        )
    };
    if let Some(composed) = &composed {
        annotate_composed_search_recency(&mut result.matches, &args.directory, composed);
    } else {
        annotate_search_recency(&mut result.matches, &args.directory);
    }
    let render_started = crate::timing::start();
    let rendered = if args.json {
        if let Some(composed) = &composed {
            output::render_find_json_with_composed(&result, args.explain, composed)
        } else {
            output::render_find_json(&result, args.explain)
        }
    } else {
        output::render_find_human(&result, args.explain)
    };
    crate::timing::emit_since(
        "cli.response_serialize",
        render_started,
        &[("matches", result.matches.len() as u64), ("bytes", rendered.len() as u64)],
    );
    emit(rendered);
    // An empty result is a valid outcome, not an error.
    EXIT_OK
}

pub struct DiagnoseArgs {
    pub query: String,
    pub target: String,
    pub directory: String,
    pub artifact_type: Option<String>,
    pub tags: Vec<String>,
    pub surface_limit: usize,
    pub json: bool,
    pub top_level: bool,
    pub live: bool,
}

/// Named-target explain-miss diagnostic. It calls the same directory-backed
/// matcher and ranking path as `find`, then renders only the trace.
pub fn cmd_diagnose(args: &DiagnoseArgs) -> i32 {
    if !Path::new(&args.directory).is_dir() {
        return usage_error(&format!("not a directory: {}", args.directory));
    }
    let composed = match load_composed_or_exit(&args.directory, !args.top_level) {
        Ok(composed) => composed,
        Err(code) => return code,
    };
    let diagnosis = if let Some(composed) = &composed {
        let identity = composed.identity_index();
        let mut effective = composed.effective_index();
        for entry in &mut effective {
            let Some(key) = &entry.key else { continue };
            if let Some(identity_entry) = identity
                .iter()
                .find(|candidate| candidate.key.as_ref() == Some(key))
            {
                entry.aliases = identity_entry.aliases.clone();
            }
        }
        let target_is_effective = composed
            .resolve(crate::pycompat::py_strip(&args.target))
            .ok()
            .is_some_and(|target| effective.iter().any(|entry| entry.key.as_ref() == Some(&target.key)));
        crate::resolve::diagnose_index(
            if target_is_effective { &effective } else { &identity },
            &args.query,
            &args.target,
            args.artifact_type.as_deref(),
            &args.tags,
            args.live,
            args.surface_limit,
        )
    } else {
        crate::resolve::diagnose_artifact(
            &args.directory,
            &args.query,
            &args.target,
            crate::resolve::DiagnoseOptions {
                artifact_type: args.artifact_type.as_deref(),
                recursive: !args.top_level,
                tags: &args.tags,
                live_only: args.live,
                surface_limit: args.surface_limit,
            },
        )
    };
    if args.json {
        emit(if composed.is_some() {
            output::render_diagnosis_json_with_origin(&diagnosis)
        } else {
            output::render_diagnosis_json(&diagnosis)
        });
    } else {
        emit(output::render_diagnosis_human(&diagnosis));
    }
    if diagnosis.outcome == crate::resolve::DIAGNOSIS_COMPLETE {
        EXIT_OK
    } else {
        EXIT_VALIDATION_FAILED
    }
}

pub struct RetrieveArgs {
    pub task: String,
    pub directory: String,
    pub scope: Option<String>,
    pub top_k: i64,
    pub budget: i64,
    pub all: bool,
    pub json: bool,
}

/// `cmd_retrieve` — one-call compound grounding retrieval (ADR-113). The
/// `--json` face emits the budget-capped serialization; the human face renders
/// the same truncated payload. An empty `items` list is a valid answer.
pub fn cmd_retrieve(args: &RetrieveArgs) -> i32 {
    if !Path::new(&args.directory).is_dir() {
        return usage_error(&format!("not a directory: {}", args.directory));
    }
    if args.top_k < 1 {
        return usage_error(&format!("--top-k must be at least 1, got {}", args.top_k));
    }
    if args.budget < 1 {
        return usage_error(&format!("--budget must be at least 1, got {}", args.budget));
    }
    let composed = match load_composed_or_exit(&args.directory, true) {
        Ok(composed) => composed,
        Err(code) => return code,
    };
    let payload = if let Some(composed) = &composed {
        crate::retrieve::retrieve_grounding_from_composed(
            &args.directory,
            &args.task,
            args.scope.as_deref(),
            args.top_k,
            args.budget,
            !args.all,
            composed,
        )
    } else {
        crate::retrieve::retrieve_grounding(
            &args.directory,
            &args.task,
            args.scope.as_deref(),
            args.top_k,
            args.budget,
            !args.all,
        )
    };
    let serialized = crate::budget::serialize(&payload, args.budget);
    if args.json {
        emit(serialized);
    } else {
        // The oracle renders from json.loads(serialized) — the truncated shape.
        let truncated: serde_json::Value =
            serde_json::from_str(&serialized).expect("serialized payload is valid JSON");
        emit(output::render_retrieve_human(&truncated));
    }
    EXIT_OK
}

// ---------------------------------------------------------------------------
// cmd_mcp_stats / cmd_usage / cmd_telemetry (local-state reporting,
// ADR-040/041/046, ADR-086 — PORT-CONTRACT.d/14)
// ---------------------------------------------------------------------------

/// The oracle CRASHES on a non-UTF-8 state log (`read_text` raises
/// `UnicodeDecodeError`; the readers catch only `OSError`): traceback to
/// stderr, EMPTY stdout, exit 1. Bug-for-bug mirror; the stderr text is
/// out of parity scope.
fn state_log_crash() -> i32 {
    eprintln!("decided-rs: state log is not valid UTF-8");
    EXIT_VALIDATION_FAILED
}

pub struct McpStatsArgs {
    pub json: bool,
    pub share: bool,
}

/// `decided mcp-stats` — Guide-only read-back. An empty or missing log is a
/// valid answer (telemetry is off by default), like `find` with no
/// matches: exit 0 for every log state.
pub fn cmd_mcp_stats(args: &McpStatsArgs) -> i32 {
    let summary = match crate::telemetry::summarize() {
        Ok(summary) => summary,
        Err(_) => return state_log_crash(),
    };
    if args.share {
        emit(crate::telemetry::share_url(&summary));
    } else if args.json {
        emit(output::render_mcp_stats_json(&summary));
    } else {
        emit(output::render_mcp_stats_human(&summary));
    }
    EXIT_OK
}

pub struct UsageArgs {
    pub json: bool,
    pub share: bool,
}

/// `decided usage` — unified read-back over the CLI-usage log and the Guide
/// log (ADR-046). No consent gate on reads; exit 0 for every log state.
/// The CLI log is read FIRST (a bad usage log crashes before the Guide
/// log is touched, like the oracle's statement order).
pub fn cmd_usage(args: &UsageArgs) -> i32 {
    let summary = match crate::usage::summarize_usage() {
        Ok(summary) => summary,
        Err(_) => return state_log_crash(),
    };
    let guide = match crate::telemetry::summarize() {
        Ok(guide) => guide,
        Err(_) => return state_log_crash(),
    };
    if args.share {
        emit(crate::usage::share_url(&summary, &guide));
    } else if args.json {
        emit(output::render_usage_json(&summary, &guide));
    } else {
        emit(output::render_usage_human(&summary, &guide));
    }
    EXIT_OK
}

pub struct SkillArgs {
    /// Validated positional choice: `install` or `list`.
    pub action: String,
    /// Optional skill name (install: one skill; absent: all, all-or-nothing).
    pub name: Option<String>,
    /// Target directory (argparse default ".").
    pub dir: String,
    pub json: bool,
}

/// `decided skill <action> [name] [--dir DIR] [--json]` — list or install the
/// bundled Claude Code agent skills. The `--dir` not-a-directory check runs
/// BEFORE the unknown-name check (skill brief, landmine 5).
pub fn cmd_skill(args: &SkillArgs) -> i32 {
    use crate::skill::{install_skills, install_targets, SkillInstallError};

    if args.action == "list" {
        if args.name.is_some() {
            return usage_error("skill list takes no skill name");
        }
        if args.json {
            emit(output::render_skill_list_json());
        } else {
            emit(output::render_skill_list_human());
        }
        return EXIT_OK;
    }

    if !Path::new(&args.dir).is_dir() {
        return usage_error(&format!("not a directory: {}", args.dir));
    }
    let targets = match install_targets(&args.dir, args.name.as_deref()) {
        Ok(targets) => targets,
        Err(SkillInstallError::NotFound(message)) => return usage_error(&message),
        Err(SkillInstallError::FileExists(message)) | Err(SkillInstallError::Io(message)) => {
            eprintln!("decided: {message}");
            return EXIT_VALIDATION_FAILED;
        }
    };
    if let Some(code) = refuse_read_only_targets(targets.iter().map(String::as_str)) {
        return code;
    }
    let installation = match install_skills(&args.dir, args.name.as_deref()) {
        Ok(installation) => installation,
        Err(SkillInstallError::NotFound(message)) => return usage_error(&message),
        Err(SkillInstallError::FileExists(message)) | Err(SkillInstallError::Io(message)) => {
            // Refused (never overwrites) or operational failure — exit 1
            // with the `decided: ` prefix, every existing file untouched.
            eprintln!("decided: {message}");
            return EXIT_VALIDATION_FAILED;
        }
    };
    if args.json {
        emit(output::render_skill_install_json(&installation));
    } else {
        emit(output::render_skill_install_human(&installation));
    }
    EXIT_OK
}

pub struct HookArgs {
    /// Validated positional choice: `install` or `list`.
    pub action: String,
    /// Validated `--style` choice (argparse default `post-commit`).
    pub style: String,
    /// Target directory (argparse default ".").
    pub dir: String,
    pub json: bool,
}

/// `decided hook <action> [--style STYLE] [--dir DIR] [--json]` — list or
/// install the bundled git hooks. `list` ignores `--style`/`--dir`; an
/// invalid style never reaches here (argparse choices fire first).
pub fn cmd_hook(args: &HookArgs) -> i32 {
    use crate::hook::{install_hook, install_target, HookInstallError};

    if args.action == "list" {
        if args.json {
            emit(output::render_hook_list_json());
        } else {
            emit(output::render_hook_list_human());
        }
        return EXIT_OK;
    }

    if !Path::new(&args.dir).is_dir() {
        return usage_error(&format!("not a directory: {}", args.dir));
    }
    let target = match install_target(&args.dir, &args.style) {
        Ok(target) => target,
        Err(HookInstallError::NotAGitWorkTree(message)) => return usage_error(&message),
        Err(HookInstallError::FileExists(message)) | Err(HookInstallError::Io(message)) => {
            eprintln!("decided: {message}");
            return EXIT_VALIDATION_FAILED;
        }
    };
    if let Some(code) = refuse_read_only_target(&target) {
        return code;
    }
    let installation = match install_hook(&args.dir, &args.style) {
        Ok(installation) => installation,
        Err(HookInstallError::NotAGitWorkTree(message)) => return usage_error(&message),
        Err(HookInstallError::FileExists(message)) | Err(HookInstallError::Io(message)) => {
            eprintln!("decided: {message}");
            return EXIT_VALIDATION_FAILED;
        }
    };
    if args.json {
        emit(output::render_hook_install_json(&installation));
    } else {
        emit(output::render_hook_install_human(&installation));
    }
    EXIT_OK
}

pub struct EvalArgs {
    pub check: bool,
    pub update_baseline: bool,
    pub json: bool,
    pub root: String,
    pub queries: String,
    pub baseline: String,
    pub config: String,
}

/// `decided eval [--check | --update-baseline] [--json] ...` — score retrieval
/// against the fixture benchmark, or gate against the baseline (ADR-066).
/// Modes win over `--json` (eval brief, landmine 7); every `EvalUsageError`
/// exits 2 with a `decided eval: ` stderr prefix — including a missing baseline
/// under `--check`, discovered only AFTER the benchmark has run (statement
/// order mirrors the oracle's single try block).
pub fn cmd_eval(args: &EvalArgs) -> i32 {
    use crate::eval;

    let fail = |err: eval::EvalUsageError| -> i32 {
        eprintln!("decided eval: {}", err.0);
        EXIT_USAGE
    };
    if args.update_baseline {
        if let Some(code) = refuse_read_only_target(&args.baseline) {
            return code;
        }
    }
    let scorecard = match eval::run_eval(&args.root, &args.queries) {
        Ok(scorecard) => scorecard,
        Err(err) => return fail(err),
    };
    if args.update_baseline {
        let payload = eval::render_metrics_json(&scorecard.metrics) + "\n";
        if let Err(e) = std::fs::write(&args.baseline, payload) {
            // The oracle lets the OSError escape as a traceback (exit 1);
            // fail with the same code without the traceback noise.
            eprintln!("decided: cannot write {}: {e}", args.baseline);
            return EXIT_VALIDATION_FAILED;
        }
        emit(format!("decided eval: baseline updated -> {}", args.baseline));
        return EXIT_OK;
    }
    if args.check {
        let baseline = match eval::load_baseline(&args.baseline) {
            Ok(baseline) => baseline,
            Err(err) => return fail(err),
        };
        let config = match eval::load_config(&args.config) {
            Ok(config) => config,
            Err(err) => return fail(err),
        };
        let failures = eval::evaluate_gate(&scorecard.metrics, &baseline, &config);
        if !failures.is_empty() {
            for failure in &failures {
                emit(failure.render());
            }
            return EXIT_VALIDATION_FAILED;
        }
        emit("decided eval: gate PASS".to_string());
        return EXIT_OK;
    }
    if args.json {
        emit(eval::render_scorecard_json(&scorecard));
    } else {
        emit(eval::render_scorecard_human(&scorecard));
    }
    EXIT_OK
}

// ---------------------------------------------------------------------------
// cmd_new / cmd_init / cmd_quickstart / cmd_migrate / cmd_rename
// (scaffold writes — PORT-CONTRACT.d/16)
// ---------------------------------------------------------------------------

pub struct NewArgs {
    pub artifact_type: String,
    pub output_path: String,
    pub json: bool,
}

/// `decided new <type> <output_path>` — create one artifact from its canonical
/// template. Usage errors (bad type, exists, missing parent, no repo
/// config) exit 2; operational errors (malformed config, id exhaustion)
/// exit 1 — all stderr `decided: <msg>`.
pub fn cmd_new(args: &NewArgs) -> i32 {
    use crate::scaffold::ScaffoldError;
    if let Some(code) = refuse_read_only_target(&args.output_path) {
        return code;
    }
    let created = match crate::scaffold::create_artifact(&args.artifact_type, &args.output_path) {
        Ok(created) => created,
        Err(
            e @ (ScaffoldError::TemplateNotFound(_)
            | ScaffoldError::OutputPathExists(_)
            | ScaffoldError::OutputDirectoryMissing(_)
            | ScaffoldError::MissingRepositoryConfig(_)),
        ) => return usage_error(e.message()),
        Err(e) => {
            eprintln!("decided: {}", e.message());
            return EXIT_VALIDATION_FAILED;
        }
    };
    if args.json {
        emit(output::render_new_json(&created));
    } else {
        emit(output::render_new_human(&created));
    }
    EXIT_OK
}

/// `_maybe_ask_usage_sharing()` — the CLI's only interactive prompt
/// (ADR-041): a real TTY on BOTH ends, no prior answer; either answer is
/// persisted so the question is asked at most once per machine. Under the
/// parity harness stdio is piped, so this never fires there; the gate and
/// bytes are mirrored for real-TTY runs and the answer handling is
/// unit-tested below.
fn maybe_ask_usage_sharing() {
    use std::io::{BufRead, IsTerminal, Write};
    if !(std::io::stdin().is_terminal() && std::io::stdout().is_terminal())
        || crate::consent::consent_recorded()
    {
        return;
    }
    {
        let mut out = std::io::stdout().lock();
        let _ = out.write_all("\nShare anonymous usage to help shape AsDecided? [y/N] ".as_bytes());
        let _ = out.flush();
    }
    let mut answer = String::new();
    let _ = std::io::stdin().lock().read_line(&mut answer); // EOF -> empty
    if let Some(message) = handle_share_answer(&answer) {
        emit(message.to_string());
    }
}

/// The prompt's answer handling: `y`/`yes` (trimmed, lowercased) opts in
/// and returns the confirmation line; anything else (including EOF/empty)
/// declines silently.
fn handle_share_answer(answer: &str) -> Option<&'static str> {
    if share_answer_is_yes(answer) {
        crate::consent::opt_in();
        Some(
            "Sharing preference recorded locally. This native build has no outbound \
             telemetry sender; 'decided telemetry status' shows the local state.",
        )
    } else {
        crate::consent::decline();
        None
    }
}

/// `answer.strip().lower() in ("y", "yes")` — the pure classification the
/// prompt applies (unit-tested; the prompt itself is TTY-gated and outside
/// the piped parity harness's reach).
fn share_answer_is_yes(answer: &str) -> bool {
    matches!(
        crate::pycompat::py_strip(answer).to_lowercase().as_str(),
        "y" | "yes"
    )
}

#[cfg(test)]
#[allow(clippy::items_after_test_module)]
mod share_prompt_tests {
    use super::share_answer_is_yes;

    /// The ADR-041 prompt accepts exactly y/yes (any case, surrounding
    /// whitespace stripped); empty input and EOF mean No.
    #[test]
    fn share_answer_classification() {
        for yes in ["y", "Y", "yes", "YES", "  y  ", "Yes\n"] {
            assert!(share_answer_is_yes(yes), "{yes:?} should opt in");
        }
        for no in ["", "\n", "n", "no", "yess", "y e s", "ok"] {
            assert!(!share_answer_is_yes(no), "{no:?} should decline");
        }
    }
}

pub struct InitArgs {
    pub directory: String,
    pub key: String,
    /// Whether `--key` was present, distinct from its released default value.
    pub key_explicit: bool,
    /// argparse-choice-validated ticketing provider.
    pub ticketing: Option<String>,
    /// argparse-choice-validated profile name.
    pub profile: Option<String>,
    /// Org endpoint URL (ADR-117); http(s)-validated in the service layer.
    pub org_endpoint: Option<String>,
    /// Emit deterministic v2-first setup guidance for materialised parents.
    pub parent_corpus: bool,
    pub json: bool,
}

/// `decided init [directory] [--key KEY] [--ticketing PROVIDER] [--profile
/// NAME] [--parent-corpus]` — establish (or confirm) the repository identity
/// namespace.
/// Invalid key exits 2; conflict/malformed config exit 1. A successful
/// non-JSON init may ask the one-time sharing question (TTY-gated).
pub fn cmd_init(args: &InitArgs) -> i32 {
    use crate::scaffold::ScaffoldError;
    if !Path::new(&args.directory).is_dir() {
        return usage_error(&format!("not a directory: {}", args.directory));
    }
    if let Some(code) = refuse_read_only_target(&args.directory) {
        return code;
    }
    let repository_key = if args.parent_corpus
        && !args.key_explicit
        && Path::new(&args.directory)
            .join(".decided/config.yaml")
            .is_file()
    {
        match crate::scaffold::load_repository_config(&args.directory) {
            Ok(Some(config)) => config.repository_key,
            Ok(None) => args.key.clone(),
            Err(error) => {
                eprintln!("decided: {}", error.message());
                return EXIT_VALIDATION_FAILED;
            }
        }
    } else {
        args.key.clone()
    };
    let result = match crate::scaffold::init_repository(
        &args.directory,
        &repository_key,
        args.ticketing.as_deref(),
        args.profile.as_deref(),
        args.org_endpoint.as_deref(),
    ) {
        Ok(result) => result,
        Err(
            e @ (ScaffoldError::InvalidRepositoryKey(_) | ScaffoldError::InvalidOrgEndpoint(_)),
        ) => return usage_error(e.message()),
        Err(e) => {
            eprintln!("decided: {}", e.message());
            return EXIT_VALIDATION_FAILED;
        }
    };
    if args.json {
        emit(output::render_init_json_with_parent_corpus(
            &result,
            args.parent_corpus,
        ));
    } else {
        emit(output::render_init_human_with_parent_corpus(
            &result,
            args.parent_corpus,
        ));
        maybe_ask_usage_sharing();
    }
    EXIT_OK
}

pub struct QuickstartArgs {
    pub directory: String,
    pub key: String,
    /// Free-string starter type (validated by the template registry).
    pub artifact_type: String,
    pub json: bool,
}

/// `decided quickstart [directory] [--key KEY] [--type TYPE]` — identity plus
/// one starter artifact in one step (ADR-044). Exit routing mirrors the
/// oracle's except ladder: bad type / bad key / missing parent are usage
/// (2); a non-empty corpus, key conflict, or occupied starter path are
/// refusals (1); operational errors are 1.
pub fn cmd_quickstart(args: &QuickstartArgs) -> i32 {
    use crate::scaffold::ScaffoldError;
    if !Path::new(&args.directory).is_dir() {
        return usage_error(&format!("not a directory: {}", args.directory));
    }
    if let Some(code) = refuse_read_only_target(&args.directory) {
        return code;
    }
    let result =
        match crate::scaffold::quickstart(&args.directory, &args.key, &args.artifact_type) {
            Ok(result) => result,
            Err(
                e @ (ScaffoldError::TemplateNotFound(_)
                | ScaffoldError::InvalidRepositoryKey(_)
                | ScaffoldError::OutputDirectoryMissing(_)),
            ) => return usage_error(e.message()),
            Err(e) => {
                eprintln!("decided: {}", e.message());
                return EXIT_VALIDATION_FAILED;
            }
        };
    if args.json {
        emit(output::render_quickstart_json(&result));
    } else {
        emit(output::render_quickstart_human(&result));
        maybe_ask_usage_sharing();
    }
    EXIT_OK
}

pub struct MigrateArgs {
    /// Validated positional choice (only `metadata` exists).
    pub target: String,
    pub directory: String,
    pub dry_run: bool,
    pub top_level: bool,
    pub json: bool,
}

/// `decided migrate metadata <directory> [--dry-run]` — canonical frontmatter
/// identity for every recognized legacy artifact. A completed migration
/// (or dry run) always exits 0 — nothing to migrate is a valid outcome.
pub fn cmd_migrate(args: &MigrateArgs) -> i32 {
    use crate::scaffold::ScaffoldError;
    if !Path::new(&args.directory).is_dir() {
        return usage_error(&format!("not a directory: {}", args.directory));
    }
    if let Some(code) = refuse_read_only_target(&args.directory) {
        return code;
    }
    if args.target == "layout" {
        return migrate_layout(args);
    }
    let report = match crate::scaffold::migrate_metadata(
        &args.directory,
        args.dry_run,
        !args.top_level,
    ) {
        Ok(report) => report,
        Err(e @ ScaffoldError::MissingRepositoryConfig(_)) => return usage_error(e.message()),
        Err(e) => {
            eprintln!("decided: {}", e.message());
            return EXIT_VALIDATION_FAILED;
        }
    };
    if args.json {
        emit(output::render_migrate_json(&report));
    } else {
        emit(output::render_migrate_human(&report));
    }
    EXIT_OK
}

/// Explicit one-way repository layout cutover. Nothing is inferred or moved
/// during ordinary commands: operators first inspect `--dry-run`, then apply.
fn migrate_layout(args: &MigrateArgs) -> i32 {
    let root = Path::new(&args.directory);
    let moves = [
        (root.join(".rac"), root.join(".decided")),
        (root.join("rac"), root.join("decisions")),
    ];
    let planned: Vec<_> = moves
        .iter()
        .filter(|(from, _)| from.exists())
        .collect();
    for (_, to) in &planned {
        if to.exists() {
            return usage_error(&format!(
                "refusing layout migration because destination already exists: {}",
                to.display()
            ));
        }
    }
    if !args.dry_run {
        for (from, to) in &planned {
            if let Err(error) = std::fs::rename(from, to) {
                eprintln!(
                    "decided: cannot migrate {} to {}: {error}",
                    from.display(),
                    to.display()
                );
                return EXIT_VALIDATION_FAILED;
            }
        }
    }
    if args.json {
        let operations: Vec<_> = planned
            .iter()
            .map(|(from, to)| {
                serde_json::json!({"from": from, "to": to})
            })
            .collect();
        emit(
            serde_json::to_string_pretty(&serde_json::json!({
                "directory": args.directory,
                "dry_run": args.dry_run,
                "operations": operations,
            }))
            .expect("layout migration result is serializable"),
        );
    } else if planned.is_empty() {
        emit("No legacy .rac or rac layout found.".to_string());
    } else {
        let verb = if args.dry_run { "Would move" } else { "Moved" };
        for (from, to) in planned {
            emit(format!("{verb} {} -> {}", from.display(), to.display()));
        }
    }
    EXIT_OK
}

pub struct RenameArgs {
    pub old: String,
    pub new: String,
    pub directory: String,
    pub apply: bool,
    pub top_level: bool,
    pub json: bool,
}

/// `decided rename <old> <new> <directory> [--apply] [--top-level]` — compute
/// (and optionally apply) the corpus-wide rename edit set. Refusals exit 1
/// with the human rendering on STDERR but the JSON plan on STDOUT; a valid
/// dry run and a successful apply exit 0.
pub fn cmd_rename(args: &RenameArgs) -> i32 {
    if !Path::new(&args.directory).is_dir() {
        return usage_error(&format!("not a directory: {}", args.directory));
    }
    if let Some(code) = refuse_read_only_target(&args.directory) {
        return code;
    }
    let composed = match load_composed_or_exit(&args.directory, !args.top_level) {
        Ok(Some(composed)) => {
            if composed
                .resolve(crate::pycompat::py_strip(&args.old))
                .ok()
                .is_some_and(|item| item.origin.layer == crate::corpus::Layer::Inherited)
            {
                eprintln!(
                    "decided: refusing to rename inherited read-only artifact {}",
                    args.old
                );
                return EXIT_VALIDATION_FAILED;
            }
            Some(composed)
        }
        Ok(None) => None,
        Err(code) => return code,
    };
    let plan = if let Some(corpus) = &composed {
        let local = crate::federated_corpus::local_writable_projection(
            &args.directory,
            corpus,
        );
        crate::rename::compute_rename_from_items(
            &args.directory,
            &args.old,
            &args.new,
            !args.top_level,
            &local,
        )
    } else {
        crate::rename::compute_rename(&args.directory, &args.old, &args.new, !args.top_level)
    };

    if !plan.ok {
        if args.json {
            emit(output::render_rename_json(&plan));
        } else {
            eprintln!("{}", output::render_rename_human(&plan));
        }
        return EXIT_VALIDATION_FAILED;
    }

    if !args.apply {
        if args.json {
            emit(output::render_rename_json(&plan));
        } else {
            emit(output::render_rename_human(&plan));
        }
        return EXIT_OK;
    }

    let result = match crate::rename::apply_rename(&plan) {
        Ok(result) => result,
        Err(message) => {
            // The oracle's stale-plan ValueError escapes as a traceback
            // (exit 1, empty stdout); same code, readable stderr.
            eprintln!("{message}");
            return EXIT_VALIDATION_FAILED;
        }
    };
    if args.json {
        emit(output::render_rename_result_json(&result));
    } else {
        emit(output::render_rename_result_human(&result));
    }
    EXIT_OK
}

pub struct CorpusDigestArgs {
    pub root: String,
    pub corpus: String,
    pub version: u32,
}

/// Read-only operator calculation for the canonical parent corpus pin. The
/// implementation consumes only local bytes below `root` and cannot write,
/// fetch, refresh, or repin anything.
pub fn cmd_corpus_digest(args: &CorpusDigestArgs) -> i32 {
    let result = if args.version == 2 {
        crate::federation::calculate_parent_digest_v2(&args.root, &args.corpus)
            .map(|result| result.digest)
    } else {
        crate::federation::calculate_parent_digest(&args.root, &args.corpus)
            .map(|result| result.digest)
    };
    match result {
        Ok(digest) => {
            emit(digest);
            EXIT_OK
        }
        Err(error) => {
            eprintln!("decided: {error}");
            EXIT_VALIDATION_FAILED
        }
    }
}

pub struct CorpusStatusArgs {
    pub directory: String,
    pub json: bool,
}

/// Verify the complete version-2 closure and render its retained topology.
/// Loading is byte-identical to every other graph consumer and remains
/// offline/read-only; success therefore means every displayed pin and route
/// was checked during this invocation.
pub fn cmd_corpus_status(args: &CorpusStatusArgs) -> i32 {
    if !Path::new(&args.directory).is_dir() {
        return usage_error(&format!("not a directory: {}", args.directory));
    }
    let corpus = match crate::federated_corpus::load_verified_graph_corpus(&args.directory) {
        Ok(Some(corpus)) => corpus,
        Ok(None) => {
            eprintln!(
                "decided: corpus status requires a manifest version 2 federation at {}",
                args.directory
            );
            return EXIT_VALIDATION_FAILED;
        }
        Err(error) => {
            eprintln!("decided: {error}");
            return EXIT_VALIDATION_FAILED;
        }
    };
    let report = crate::federation_observability::FederationStatusReport::from_corpus(&corpus);
    emit(if args.json {
        report.render_json()
    } else {
        report.render_human()
    });
    EXIT_OK
}

pub struct CorpusExplainArgs {
    pub reference: String,
    pub directory: String,
    pub context: Option<String>,
    pub json: bool,
}

/// Explain one source-contextual graph lookup, including historical
/// candidates, the effective terminal, and explicit override provenance.
pub fn cmd_corpus_explain(args: &CorpusExplainArgs) -> i32 {
    if !Path::new(&args.directory).is_dir() {
        return usage_error(&format!("not a directory: {}", args.directory));
    }
    let corpus = match crate::federated_corpus::load_verified_graph_corpus(&args.directory) {
        Ok(Some(corpus)) => corpus,
        Ok(None) => {
            eprintln!(
                "decided: corpus explain requires a manifest version 2 federation at {}",
                args.directory
            );
            return EXIT_VALIDATION_FAILED;
        }
        Err(error) => {
            eprintln!("decided: {error}");
            return EXIT_VALIDATION_FAILED;
        }
    };
    let context = args
        .context
        .as_deref()
        .unwrap_or_else(|| corpus.composition.root_source());
    let report = crate::federation_observability::FederationExplainReport::from_corpus(
        &corpus,
        context,
        &args.reference,
    );
    let ok = report.ok();
    emit(if args.json {
        report.render_json()
    } else {
        report.render_human()
    });
    if ok {
        EXIT_OK
    } else {
        EXIT_VALIDATION_FAILED
    }
}

pub struct TelemetryArgs {
    /// Validated positional choice; argparse default is `status`.
    pub action: String,
    pub enterprise: bool,
    pub unlock: bool,
}

/// `decided telemetry [on|off|status] [--enterprise] [--unlock]` — show or
/// change sharing consent (ADR-041) and the enterprise hard-lock
/// (ADR-086). Flag validation order is pinned: enterprise/unlock with a
/// non-`off` action first, then unlock-without-enterprise, then the
/// opt-in-while-locked refusal — three distinct exit-2 usage errors.
pub fn cmd_telemetry(args: &TelemetryArgs) -> i32 {
    if (args.enterprise || args.unlock) && args.action != "off" {
        return usage_error("--enterprise/--unlock are only valid with 'decided telemetry off'");
    }
    if args.unlock && !args.enterprise {
        return usage_error(
            "--unlock requires --enterprise (use 'decided telemetry off --enterprise --unlock')",
        );
    }

    if args.action == "on" {
        if crate::consent::load_consent().enterprise_locked {
            return usage_error(
                "cannot opt in while the enterprise telemetry lock is set; remove it with \
                 'decided telemetry off --enterprise --unlock' first (ADR-086).",
            );
        }
        let record = crate::consent::opt_in();
        emit(format!(
            "Sharing preference recorded locally. Install id: {}",
            record.install_id
        ));
        emit(
            "This native build has no outbound telemetry sender; nothing is sent. \
             Local usage read-back and explicit share URLs remain available (ADR-131)."
                .to_string(),
        );
        #[allow(clippy::const_is_empty)]
        if crate::consent::POSTHOG_API_KEY.is_empty() {
            emit("Endpoint key: not configured — nothing is sent.".to_string());
        }
    } else if args.action == "off" {
        if args.enterprise && args.unlock {
            crate::consent::enterprise_unlock();
            emit(
                "Enterprise lock removed. Sharing stays off; re-enable with \
                 'decided telemetry on' (ADR-086)."
                    .to_string(),
            );
        } else if args.enterprise {
            crate::consent::enterprise_lock();
            emit(
                "Sharing off and enterprise-locked. Outbound sharing is disabled \
                 and cannot be re-enabled until unlocked with \
                 'decided telemetry off --enterprise --unlock' (ADR-086)."
                    .to_string(),
            );
        } else {
            crate::consent::opt_out();
            emit("Sharing off. Nothing will be sent.".to_string());
        }
    } else {
        // status — `Sharing:` tri-state precedence: the enterprise lock
        // wins over sharing; the 5th line is locked-note XOR sharing-note.
        let status = crate::consent::consent_status();
        let sharing = if status.enterprise_locked {
            "locked (enterprise)"
        } else if status.sharing {
            "on"
        } else {
            "off"
        };
        emit(format!("Sharing: {sharing}"));
        emit(format!(
            "Install id: {}",
            if status.install_id.is_empty() {
                "(none)"
            } else {
                &status.install_id
            }
        ));
        emit(format!(
            "Consented at: {}",
            if status.consented_at.is_empty() {
                "(never)"
            } else {
                &status.consented_at
            }
        ));
        emit(format!("Consent file: {}", status.path));
        if status.enterprise_locked {
            emit(
                "Enterprise lock: on \u{2014} outbound sharing is disabled. Remove with \
                 'decided telemetry off --enterprise --unlock' (ADR-086)."
                    .to_string(),
            );
        } else if status.sharing {
            emit(
                "Local sharing preference: enabled. This native build has no outbound \
                 telemetry sender; no paths, queries, or content leave the machine (ADR-131)."
                    .to_string(),
            );
        }
        if !status.endpoint_configured {
            emit("Endpoint key: not configured \u{2014} nothing is sent.".to_string());
        }
    }
    EXIT_OK
}

#[cfg(test)]
mod validation_provenance_tests {
    use std::fs;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    static COUNTER: AtomicUsize = AtomicUsize::new(0);

    fn scratch() -> PathBuf {
        let count = COUNTER.fetch_add(1, Ordering::SeqCst);
        let root = std::env::temp_dir().join(format!(
            "asdecided-validation-provenance-{}-{count}",
            std::process::id()
        ));
        fs::create_dir_all(root.join(".decided")).unwrap();
        fs::create_dir_all(root.join("decisions")).unwrap();
        root
    }

    fn validation(origin: Option<ArtifactOrigin>) -> DirectoryValidation {
        DirectoryValidation {
            directory: "decisions".to_string(),
            recursive: true,
            files: vec![FileValidation {
                path: "decisions/example.md".to_string(),
                artifact_type: "Decision".to_string(),
                status: STATUS_INVALID,
                issues: vec![Issue::new(
                    "error",
                    "missing-title",
                    "Title is required.".to_string(),
                    Some(3),
                )],
                origin,
                source_route: None,
                route_count: None,
            }],
            okf: None,
        }
    }

    #[test]
    fn composed_validation_adds_machine_provenance_only() {
        let legacy = validation(None);
        let composed = validation(Some(
            CorpusLayer::inherited(
                "acme/standards",
                "standards",
                "sha256:0123456789abcdef",
            )
            .origin(),
        ));

        assert_eq!(
            output::render_validate_dir_human(&legacy),
            output::render_validate_dir_human(&composed)
        );

        let legacy_json: serde_json::Value =
            serde_json::from_str(&output::render_validate_dir_json(&legacy)).unwrap();
        assert!(legacy_json["files"][0].get("provenance").is_none());
        let composed_json: serde_json::Value =
            serde_json::from_str(&output::render_validate_dir_json(&composed)).unwrap();
        assert_eq!(
            composed_json["files"][0]["provenance"],
            serde_json::json!({
                "layer": "inherited",
                "pin": "sha256:0123456789abcdef",
                "source": "acme/standards",
            })
        );

        let legacy_sarif: serde_json::Value =
            serde_json::from_str(&output::render_validate_sarif(&legacy)).unwrap();
        assert!(legacy_sarif["runs"][0]["results"][0]
            .get("properties")
            .is_none());
        let composed_sarif: serde_json::Value =
            serde_json::from_str(&output::render_validate_sarif(&composed)).unwrap();
        assert_eq!(
            composed_sarif["runs"][0]["results"][0]["properties"],
            composed_json["files"][0]["provenance"]
        );
    }

    #[test]
    fn topology_validation_retains_route_context_in_every_renderer() {
        let result = DirectoryValidation {
            directory: "decisions".to_string(),
            recursive: true,
            files: vec![FileValidation {
                path: crate::federation::MANIFEST_RELATIVE_PATH.to_string(),
                artifact_type: "corpus-manifest".to_string(),
                status: STATUS_INVALID,
                issues: vec![Issue::new(
                    "error",
                    "corpus-federation-invalid-node",
                    "shared inherited node is invalid".to_string(),
                    None,
                )],
                origin: Some(ArtifactOrigin {
                    source: "acme/shared".to_string(),
                    layer: crate::corpus::Layer::Inherited,
                    pin: Some(format!("sha256-v2:{}", "a".repeat(64))),
                    alias: None,
                }),
                source_route: Some(vec![
                    "acme/root".to_string(),
                    "acme/left".to_string(),
                    "acme/shared".to_string(),
                ]),
                route_count: Some(2),
            }],
            okf: None,
        };

        let human = output::render_validate_dir_human(&result);
        assert!(human.contains(
            "source route: acme/root -> acme/left -> acme/shared (2 verified physical routes)"
        ));

        let json: serde_json::Value =
            serde_json::from_str(&output::render_validate_dir_json(&result)).unwrap();
        assert_eq!(
            json["files"][0]["provenance"]["source_route"],
            serde_json::json!(["acme/root", "acme/left", "acme/shared"])
        );
        assert_eq!(json["files"][0]["provenance"]["route_count"], 2);

        let sarif: serde_json::Value =
            serde_json::from_str(&output::render_validate_sarif(&result)).unwrap();
        assert_eq!(
            sarif["runs"][0]["results"][0]["properties"],
            json["files"][0]["provenance"]
        );
    }

    #[test]
    fn parsed_manifest_identity_provenances_later_load_failures() {
        let root = scratch();
        fs::write(
            root.join(".decided/config.yaml"),
            "repository_key: APP\ncorpus:\n  source: acme/app\n",
        )
        .unwrap();
        let pin = format!("sha256:{}", "0".repeat(64));
        fs::write(
            root.join(crate::federation::MANIFEST_RELATIVE_PATH),
            format!(
                "# Corpus\n\n## inherits\n\n```yaml\nversion: 1\nalias: standards\n\
                 source: acme/standards\nroot: vendor/standards\ncorpus: decisions\n\
                 digest: {pin}\n```\n"
            ),
        )
        .unwrap();

        let origin = manifest_failure_origin(&root.join("decisions").to_string_lossy()).unwrap();
        assert_eq!(origin.source, "acme/standards");
        assert_eq!(origin.layer, crate::corpus::Layer::Inherited);
        assert_eq!(origin.alias.as_deref(), Some("standards"));
        assert_eq!(origin.pin.as_deref(), Some(pin.as_str()));

        fs::remove_dir_all(root).unwrap();
    }
}
