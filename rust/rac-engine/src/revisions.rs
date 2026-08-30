//! Git revision materialization (`decided.services.revisions`) — the only
//! git-consuming module of the watchkeeper and point-in-time export paths
//! (ADR-043). The released watchkeeper seam retains its `git archive` parity
//! contract. Point-in-time export uses the stricter [`RevisionSnapshot`]
//! path, which reads bounded exact blob objects and declared submodule
//! commits without fetching, checking out, or mutating `.git`.
//!
//! Watchkeeper contract mirrored from the oracle:
//! - `git rev-parse --show-toplevel` (cwd = the corpus directory) finds the
//!   work-tree root; failure -> `not a git repository: <directory>`; a
//!   missing git binary -> `git executable not found` (both exit 2 at the
//!   CLI as `decided: <msg>`).
//! - `git rev-parse --verify --quiet <rev>^{commit}` (cwd = repo root);
//!   nonzero -> `unknown revision: <rev>`.
//! - `git archive --format=tar <rev> -- <pathspec>` (cwd = repo root); a
//!   NONZERO exit is not an error — the subpath does not exist at that
//!   revision and an EMPTY corpus is materialized (the fresh-adoption
//!   "everything added" comparison).
//! - The temporary directory is prefixed `decided-watchkeeper-` and removed
//!   when the materialization guard drops. Its path never appears in any
//!   output surface (all reported paths are corpus-relative).

use std::collections::BTreeMap;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::federation::{
    V2_MAX_FILE_BYTES, V2_MAX_INHERITANCE_DEPTH, V2_MAX_INHERITED_FILES, V2_MAX_PATH_BYTES,
    V2_MAX_PATH_COMPONENTS, V2_MAX_PHYSICAL_BYTES, V2_MAX_VISITED_ENTRIES,
};

/// The released watchkeeper revision errors. Keep this enum exhaustive-match
/// compatible for downstream callers.
#[derive(Debug)]
pub enum RevisionError {
    /// `NotAGitRepository` — not inside a git work tree, or no git binary.
    NotAGitRepository(String),
    /// `RevisionNotFound` — the name does not resolve to a commit.
    RevisionNotFound(String),
}

impl RevisionError {
    pub fn message(&self) -> &str {
        match self {
            RevisionError::NotAGitRepository(m) => m,
            RevisionError::RevisionNotFound(m) => m,
        }
    }
}

/// Errors from bounded, strict point-in-time snapshots.
#[derive(Debug)]
pub enum RevisionSnapshotError {
    NotAGitRepository(String),
    RevisionNotFound(String),
    MaterializationFailed(String),
}

impl RevisionSnapshotError {
    pub fn message(&self) -> &str {
        match self {
            RevisionSnapshotError::NotAGitRepository(message)
            | RevisionSnapshotError::RevisionNotFound(message)
            | RevisionSnapshotError::MaterializationFailed(message) => message,
        }
    }
}

impl From<RevisionError> for RevisionSnapshotError {
    fn from(error: RevisionError) -> Self {
        match error {
            RevisionError::NotAGitRepository(message) => {
                RevisionSnapshotError::NotAGitRepository(message)
            }
            RevisionError::RevisionNotFound(message) => {
                RevisionSnapshotError::RevisionNotFound(message)
            }
        }
    }
}

/// `_run_git(args, cwd)` — capture both streams, never check. Only a
/// missing binary maps to `NotAGitRepository("git executable not found")`,
/// like the oracle's `FileNotFoundError` arm.
fn run_git(args: &[&str], cwd: &Path) -> Result<Output, RevisionError> {
    let mut command = git_command(args, cwd);
    command
        .stdin(Stdio::null())
        .output()
        .map_err(|_| {
            // FileNotFoundError -> "git executable not found"; the oracle
            // would crash on any other spawn failure — degrade to the same
            // user-facing class (PORT-CONTRACT decision 3).
            RevisionError::NotAGitRepository("git executable not found".to_string())
        })
}

/// Build a read-only Git command that cannot reinterpret objects through
/// replacement refs or mutate a promisor repository by fetching objects.
fn git_command(args: &[&str], cwd: &Path) -> Command {
    let mut command = base_git_command(cwd);
    command.args(args);
    command
}

fn base_git_command(cwd: &Path) -> Command {
    let mut command = Command::new("git");
    command.current_dir(cwd);
    for (variable, _) in std::env::vars_os() {
        if variable.to_string_lossy().starts_with("GIT_") {
            command.env_remove(variable);
        }
    }
    command
        .env("GIT_NO_REPLACE_OBJECTS", "1")
        .env("GIT_NO_LAZY_FETCH", "1")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("LC_ALL", "C");
    command
}

/// `repository_root(directory)` — the work-tree root containing `directory`.
pub fn repository_root(directory: &str) -> Result<String, RevisionError> {
    let out = run_git(&["rev-parse", "--show-toplevel"], Path::new(directory))?;
    if !out.status.success() {
        return Err(RevisionError::NotAGitRepository(format!(
            "not a git repository: {directory}"
        )));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// One materialized revision: the guard owns the temporary directory and
/// removes it (best effort) on drop, like the oracle's
/// `tempfile.TemporaryDirectory` context.
pub struct MaterializedRevision {
    root: PathBuf,
    /// The corpus directory inside the temp tree (`tmp/<subpath>`).
    pub corpus: PathBuf,
}

impl Drop for MaterializedRevision {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// A fresh `decided-watchkeeper-` temp directory under the platform temp root
/// (std honors TMPDIR like `tempfile` does).
fn make_temp_dir(prefix: &str) -> io::Result<PathBuf> {
    make_temp_dir_at(&std::env::temp_dir(), prefix)
}

fn make_temp_dir_at(base: &Path, prefix: &str) -> io::Result<PathBuf> {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let pid = std::process::id();
    loop {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let candidate = base.join(format!("{prefix}-{pid}-{n}"));
        match create_private_dir(&candidate) {
            Ok(()) => return Ok(candidate),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
}

#[cfg(unix)]
fn create_private_dir(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::DirBuilderExt;

    let mut builder = std::fs::DirBuilder::new();
    builder.mode(0o700).create(path)
}

#[cfg(not(unix))]
fn create_private_dir(path: &Path) -> io::Result<()> {
    std::fs::create_dir(path)
}

/// Missing-path behavior for a selected path in a strict revision snapshot.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MissingPathPolicy {
    /// A missing selected path is a materialization error.
    Error,
    /// Report `existed = false` without creating a filesystem entry.
    Ignore,
    /// Create an empty directory and report `existed = false`.
    ///
    /// This is intentionally explicit and is used only for the historical
    /// child corpus fresh-adoption case. Archive and extraction failures are
    /// never converted into an empty directory.
    EmptyDirectory,
}

/// Symlink validation mode for a historical corpus walk.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CorpusSymlinkPolicy {
    /// Match the ordinary corpus walker: hidden and non-Markdown entries are
    /// ignored, while a visible Markdown symlink is rejected.
    CorpusFilesOnly,
    /// Match the v2 federation verifier, which rejects every symlink inside a
    /// selected node before hidden/extension filtering.
    RejectAll,
}

/// The result of materializing one selected repository-relative path.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MaterializedPath {
    /// The selected path underneath the snapshot root.
    pub path: PathBuf,
    /// Whether the selected path existed in the recorded revision.
    pub existed: bool,
}

/// A bounded, strict snapshot of selected paths from one Git revision.
///
/// Callers open one guard and then add only the config, manifest, and corpus
/// paths admitted by the federation graph. Exact paths are read from Git blob
/// objects, so `export-ignore` and `export-subst` cannot alter provenance.
/// Every Git command is read-only (`rev-parse`, `ls-tree`, `cat-file`, and
/// `config --blob`), and no operation can fetch or check out data.
pub struct RevisionSnapshot {
    root: PathBuf,
    repository_root: PathBuf,
    revision: String,
    commit: String,
    limits: CorpusLimits,
}

impl Drop for RevisionSnapshot {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

impl RevisionSnapshot {
    /// Open a strict snapshot guard for `rev` without materializing any path.
    pub fn open(repo_root: &str, rev: &str) -> Result<Self, RevisionSnapshotError> {
        let repository_root = std::fs::canonicalize(repo_root).map_err(|error| {
            RevisionSnapshotError::NotAGitRepository(format!("not a git repository: {error}"))
        })?;
        let verify = run_git(
            &[
                "rev-parse",
                "--verify",
                "--quiet",
                &format!("{rev}^{{commit}}"),
            ],
            &repository_root,
        )?;
        if !verify.status.success() {
            return Err(RevisionSnapshotError::RevisionNotFound(format!(
                "unknown revision: {rev}"
            )));
        }
        let commit = String::from_utf8_lossy(&verify.stdout).trim().to_string();
        if commit.is_empty() {
            return Err(RevisionSnapshotError::RevisionNotFound(format!(
                "unknown revision: {rev}"
            )));
        }
        let temp_base = std::fs::canonicalize(std::env::temp_dir()).map_err(|error| {
            RevisionSnapshotError::MaterializationFailed(format!(
                "cannot resolve revision snapshot base: {error}"
            ))
        })?;
        let mut protected_roots = vec![("selected repository", repository_root.clone())];
        if let Some(git_dir) = absolute_git_dir(&repository_root)? {
            protected_roots.push(("selected Git directory", git_dir));
        }
        if let Some(common_dir) = absolute_git_common_dir(&repository_root)? {
            protected_roots.push(("selected Git common directory", common_dir));
        }
        if let Some((boundary, _)) = protected_roots
            .iter()
            .find(|(_, protected)| temp_base.starts_with(protected))
        {
            return Err(RevisionSnapshotError::MaterializationFailed(format!(
                "revision snapshot base must be outside the {boundary}: {}",
                temp_base.display()
            )));
        }
        let created_root = make_temp_dir_at(&temp_base, "decided-revision").map_err(|error| {
            RevisionSnapshotError::MaterializationFailed(format!(
                "cannot create revision snapshot: {error}"
            ))
        })?;
        let root = std::fs::canonicalize(&created_root).map_err(|error| {
            let _ = std::fs::remove_dir(&created_root);
            RevisionSnapshotError::MaterializationFailed(format!(
                "cannot resolve revision snapshot directory: {error}"
            ))
        })?;
        Ok(Self {
            root,
            repository_root,
            revision: rev.to_string(),
            commit,
            limits: CorpusLimits::default(),
        })
    }

    /// Root of the temporary snapshot tree.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Strictly materialize one exact repository-relative file.
    ///
    /// Gitlink components are followed through already-local submodule object
    /// databases using the commit recorded by the containing tree. This
    /// repeats for nested gitlinks. A missing object database or recorded
    /// commit is an error: the method never fetches and never substitutes
    /// working-tree bytes for the recorded commit.
    pub fn materialize_path(
        &mut self,
        repo_relative: impl AsRef<Path>,
        missing: MissingPathPolicy,
    ) -> Result<MaterializedPath, RevisionSnapshotError> {
        let components = normalized_components(repo_relative.as_ref(), false)?;
        validate_snapshot_path(&components)?;
        let requested = components.join("/");
        let destination = join_components(&self.root, &components);
        let Some(resolved) = self.resolve_selection(&components)? else {
            return self.handle_missing(&requested, destination, missing);
        };
        let Some(entry) = resolved.entry.as_ref() else {
            return Err(materialization_error(format!(
                "revision snapshot path must select a file, not a submodule root: {requested}"
            )));
        };
        if entry.kind != "blob" {
            return Err(materialization_error(format!(
                "revision snapshot path must select a file: {requested}"
            )));
        }
        let size = entry.size.ok_or_else(|| {
            materialization_error(format!(
                "git ls-tree did not report a blob size for {requested}"
            ))
        })?;
        if size > V2_MAX_FILE_BYTES as u64 {
            return Err(materialization_error(format!(
                "revision snapshot file exceeds {V2_MAX_FILE_BYTES} bytes: {requested}"
            )));
        }
        if !self.limits.admit(&requested, entry)? {
            return Ok(MaterializedPath {
                path: destination,
                existed: true,
            });
        }
        self.limits.physical_bytes = self
            .limits
            .physical_bytes
            .checked_add(size)
            .ok_or_else(|| materialization_error("revision snapshot byte count overflow".into()))?;
        if self.limits.physical_bytes > V2_MAX_PHYSICAL_BYTES as u64 {
            return Err(materialization_error(format!(
                "revision snapshot exceeds physical byte limit {V2_MAX_PHYSICAL_BYTES}"
            )));
        }
        materialize_blob(&resolved.repository, entry, &destination, &requested)?;
        Ok(MaterializedPath {
            path: destination,
            existed: true,
        })
    }

    /// Materialize only corpus-relevant Markdown entries under one selected
    /// directory. A root corpus (`.`) is allowed here without archiving the
    /// whole repository: hidden components and non-`.md` entries are omitted
    /// with the same path semantics as the corpus walker.
    ///
    /// A missing non-root corpus becomes an empty directory with
    /// `existed = false`, as required for historical fresh adoption.
    pub fn materialize_corpus(
        &mut self,
        repo_relative: impl AsRef<Path>,
    ) -> Result<MaterializedPath, RevisionSnapshotError> {
        self.materialize_corpus_with_policy(repo_relative, MissingPathPolicy::EmptyDirectory)
    }

    /// Materialize corpus Markdown with an explicit missing-path policy.
    ///
    /// Historical child corpora use [`MissingPathPolicy::EmptyDirectory`] for
    /// fresh adoption. Declared federation parents must use
    /// [`MissingPathPolicy::Error`] so an unavailable parent cannot silently
    /// become an empty corpus.
    pub fn materialize_corpus_with_policy(
        &mut self,
        repo_relative: impl AsRef<Path>,
        missing: MissingPathPolicy,
    ) -> Result<MaterializedPath, RevisionSnapshotError> {
        self.materialize_corpus_with_options(
            repo_relative,
            missing,
            &[],
            CorpusSymlinkPolicy::CorpusFilesOnly,
        )
    }

    /// Materialize corpus Markdown while pruning repository-relative roots.
    ///
    /// The exclusions are declared federation parent roots. They are omitted
    /// from the child walk so only each parent's explicitly selected corpus is
    /// materialized by its later, required scan.
    pub fn materialize_corpus_with_policy_and_exclusions(
        &mut self,
        repo_relative: impl AsRef<Path>,
        missing: MissingPathPolicy,
        excluded_repo_relative: &[PathBuf],
    ) -> Result<MaterializedPath, RevisionSnapshotError> {
        self.materialize_corpus_with_options(
            repo_relative,
            missing,
            excluded_repo_relative,
            CorpusSymlinkPolicy::RejectAll,
        )
    }

    /// Materialize a corpus with explicit missing, exclusion, and symlink
    /// semantics. This is the closure-first v2 federation entry point.
    pub fn materialize_corpus_with_options(
        &mut self,
        repo_relative: impl AsRef<Path>,
        missing: MissingPathPolicy,
        excluded_repo_relative: &[PathBuf],
        symlink_policy: CorpusSymlinkPolicy,
    ) -> Result<MaterializedPath, RevisionSnapshotError> {
        let components = normalized_components(repo_relative.as_ref(), true)?;
        validate_snapshot_path(&components)?;
        let mut excluded_roots = Vec::new();
        for excluded in excluded_repo_relative {
            let excluded = normalized_components(excluded, false)?;
            validate_snapshot_path(&excluded)?;
            if excluded.len() < components.len() || !excluded.starts_with(&components) {
                return Err(materialization_error(format!(
                    "revision snapshot corpus exclusion must equal or be nested under {}: {}",
                    if components.is_empty() {
                        ".".to_string()
                    } else {
                        components.join("/")
                    },
                    excluded.join("/")
                )));
            }
            excluded_roots.push(excluded);
        }
        excluded_roots.sort();
        excluded_roots.dedup();
        let requested = if components.is_empty() {
            ".".to_string()
        } else {
            components.join("/")
        };
        let destination = join_components(&self.root, &components);
        let resolved = if components.is_empty() {
            ResolvedSelection {
                repository: self.repository_root.clone(),
                commit: self.commit.clone(),
                local_components: Vec::new(),
                entry: None,
            }
        } else {
            let Some(resolved) = self.resolve_selection(&components)? else {
                return self.handle_missing(&requested, destination, missing);
            };
            resolved
        };
        if resolved
            .entry
            .as_ref()
            .is_some_and(|entry| entry.kind != "tree")
        {
            return Err(materialization_error(format!(
                "corpus path is not a directory at revision {}: {requested}",
                self.revision
            )));
        }
        std::fs::create_dir_all(&destination).map_err(|error| {
            materialization_error(format!(
                "cannot create snapshot corpus {requested}: {error}"
            ))
        })?;
        if excluded_roots.iter().any(|excluded| excluded == &components) {
            return Ok(MaterializedPath {
                path: destination,
                existed: true,
            });
        }

        let mut batches: BTreeMap<(PathBuf, String), Vec<BlobTask>> = BTreeMap::new();
        let mut directories = Vec::new();
        collect_corpus_tasks(
            &resolved.repository,
            &resolved.commit,
            &resolved.local_components,
            &components,
            &self.root,
            0,
            &mut self.limits,
            &mut batches,
            &mut directories,
            &excluded_roots,
            symlink_policy,
        )?;
        for directory in directories {
            std::fs::create_dir_all(&directory).map_err(|error| {
                materialization_error(format!(
                    "cannot create Markdown-named corpus directory {}: {error}",
                    directory.display()
                ))
            })?;
        }
        for ((repository, _commit), tasks) in batches {
            materialize_blob_batch(&repository, &tasks)?;
        }
        Ok(MaterializedPath {
            path: destination,
            existed: true,
        })
    }

    fn resolve_selection(
        &self,
        components: &[String],
    ) -> Result<Option<ResolvedSelection>, RevisionSnapshotError> {
        let mut repository = self.repository_root.clone();
        let mut commit = self.commit.clone();
        let mut component_start = 0usize;
        let mut submodule_depth = 0usize;
        loop {
            let mut followed_gitlink = false;
            for component_end in component_start + 1..=components.len() {
                let local_components = components[component_start..component_end].to_vec();
                let local_path = local_components.join("/");
                let Some(entry) = tree_entry(&repository, &commit, &local_path)? else {
                    return Ok(None);
                };
                if entry.mode == "160000" {
                    submodule_depth = submodule_depth.saturating_add(1);
                    if submodule_depth > V2_MAX_INHERITANCE_DEPTH {
                        return Err(materialization_error(format!(
                            "revision snapshot exceeds nested submodule depth limit {V2_MAX_INHERITANCE_DEPTH}"
                        )));
                    }
                    if entry.kind != "commit" {
                        return Err(materialization_error(format!(
                            "invalid gitlink at {} in revision {}",
                            components[..component_end].join("/"),
                            self.revision
                        )));
                    }
                    let global_gitlink = components[..component_end].join("/");
                    let submodule = require_local_submodule(
                        &repository,
                        &commit,
                        &local_components,
                        &entry.object,
                        &global_gitlink,
                    )?;
                    repository = submodule;
                    commit = entry.object;
                    component_start = component_end;
                    followed_gitlink = true;
                    if component_start == components.len() {
                        return Ok(Some(ResolvedSelection {
                            repository,
                            commit,
                            local_components: Vec::new(),
                            entry: None,
                        }));
                    }
                    break;
                }
                if component_end == components.len() {
                    return Ok(Some(ResolvedSelection {
                        repository,
                        commit,
                        local_components,
                        entry: Some(entry),
                    }));
                }
            }
            if !followed_gitlink {
                return Ok(None);
            }
        }
    }

    fn handle_missing(
        &self,
        requested: &str,
        destination: PathBuf,
        missing: MissingPathPolicy,
    ) -> Result<MaterializedPath, RevisionSnapshotError> {
        match missing {
            MissingPathPolicy::Error => {
                return Err(materialization_error(format!(
                    "path does not exist at revision {}: {requested}",
                    self.revision
                )));
            }
            MissingPathPolicy::Ignore => {}
            MissingPathPolicy::EmptyDirectory => {
                std::fs::create_dir_all(&destination).map_err(|error| {
                    materialization_error(format!(
                        "cannot create empty snapshot path {requested}: {error}"
                    ))
                })?;
            }
        }
        Ok(MaterializedPath {
            path: destination,
            existed: false,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct TreeEntry {
    mode: String,
    kind: String,
    object: String,
    size: Option<u64>,
}

struct ResolvedSelection {
    repository: PathBuf,
    commit: String,
    local_components: Vec<String>,
    /// `None` means the selected path is the root tree of a followed gitlink.
    entry: Option<TreeEntry>,
}

#[derive(Debug)]
struct BlobTask {
    entry: TreeEntry,
    destination: PathBuf,
    display_path: String,
}

#[derive(Default)]
struct CorpusLimits {
    streamed: usize,
    unkeyed_streamed: usize,
    query_records: usize,
    visited: usize,
    files: usize,
    physical_bytes: u64,
    entries: BTreeMap<String, TreeEntry>,
    streamed_entries: BTreeMap<String, TreeEntry>,
}

impl CorpusLimits {
    fn charge_query(&mut self, context: &str) -> Result<(), RevisionSnapshotError> {
        const MAX_QUERY_MULTIPLIER: usize = 4;

        self.query_records = self.query_records.saturating_add(1);
        if self.query_records
            > V2_MAX_VISITED_ENTRIES.saturating_mul(MAX_QUERY_MULTIPLIER)
        {
            return Err(materialization_error(format!(
                "revision snapshot exceeds bounded Git query overhead for {context}"
            )));
        }
        Ok(())
    }

    fn charge_unkeyed_streamed(&mut self, context: &str) -> Result<(), RevisionSnapshotError> {
        self.unkeyed_streamed = self.unkeyed_streamed.saturating_add(1);
        if self.streamed.saturating_add(self.unkeyed_streamed) > V2_MAX_VISITED_ENTRIES {
            return Err(materialization_error(format!(
                "revision snapshot exceeds raw streamed entry limit {V2_MAX_VISITED_ENTRIES} for {context}"
            )));
        }
        Ok(())
    }

    fn charge_streamed(
        &mut self,
        path: &str,
        entry: &TreeEntry,
    ) -> Result<(), RevisionSnapshotError> {
        if let Some(previous) = self.streamed_entries.get(path) {
            if previous == entry {
                return Ok(());
            }
            return Err(materialization_error(format!(
                "conflicting streamed Git entries target revision snapshot path {path}"
            )));
        }
        self.streamed = self.streamed.saturating_add(1);
        if self.streamed.saturating_add(self.unkeyed_streamed) > V2_MAX_VISITED_ENTRIES {
            return Err(materialization_error(format!(
                "revision snapshot exceeds raw streamed entry limit {V2_MAX_VISITED_ENTRIES} for {path}"
            )));
        }
        self.streamed_entries
            .insert(path.to_string(), entry.clone());
        Ok(())
    }

    /// Admit one snapshot destination exactly once. A repeated traversal of
    /// the same committed entry is free; a different source object attempting
    /// to occupy that destination fails closed.
    fn admit(&mut self, path: &str, entry: &TreeEntry) -> Result<bool, RevisionSnapshotError> {
        if let Some(previous) = self.entries.get(path) {
            if previous == entry {
                return Ok(false);
            }
            return Err(materialization_error(format!(
                "conflicting committed entries target revision snapshot path {path}"
            )));
        }
        if self.visited >= V2_MAX_VISITED_ENTRIES {
            return Err(materialization_error(format!(
                "revision snapshot exceeds visited entry limit {V2_MAX_VISITED_ENTRIES}"
            )));
        }
        self.entries.insert(path.to_string(), entry.clone());
        self.visited += 1;
        Ok(true)
    }
}

fn materialization_error(message: String) -> RevisionSnapshotError {
    RevisionSnapshotError::MaterializationFailed(message)
}

fn command_stderr(output: &Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if stderr.is_empty() {
        format!("git exited with {}", output.status)
    } else {
        stderr
    }
}

/// Convert a platform path to validated Git path components. Joining the
/// result with `/` gives Git's required separator even on Windows.
fn normalized_components(
    path: &Path,
    allow_root: bool,
) -> Result<Vec<String>, RevisionSnapshotError> {
    let mut out = Vec::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::Normal(value) => {
                let value = value.to_str().ok_or_else(|| {
                    materialization_error("revision snapshot paths must be valid UTF-8".to_string())
                })?;
                validate_portable_component(value, &path.display().to_string())?;
                out.push(value.to_string());
            }
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(materialization_error(format!(
                    "revision snapshot path must be repository-relative: {}",
                    path.display()
                )));
            }
        }
    }
    if out.is_empty() && !allow_root {
        return Err(materialization_error(
            "revision snapshot path must select a bounded repository-relative path".to_string(),
        ));
    }
    Ok(out)
}

fn validate_portable_component(
    component: &str,
    context: &str,
) -> Result<(), RevisionSnapshotError> {
    let bytes = component.as_bytes();
    let windows_prefix = bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':';
    if component.is_empty()
        || component == "."
        || component == ".."
        || component.contains('/')
        || component.contains('\\')
        || windows_prefix
    {
        return Err(materialization_error(format!(
            "unsafe revision snapshot path component {component:?} in {context}"
        )));
    }
    Ok(())
}

fn join_components(root: &Path, components: &[String]) -> PathBuf {
    let mut out = root.to_path_buf();
    for component in components {
        out.push(component);
    }
    out
}

fn strip_component_prefix<'a>(path: &'a [String], prefix: &[String]) -> Option<&'a [String]> {
    if path.starts_with(prefix) {
        Some(&path[prefix.len()..])
    } else {
        None
    }
}

fn parse_tree_record(
    record: &[u8],
    context: &str,
) -> Result<(Vec<String>, TreeEntry), RevisionSnapshotError> {
    let Some(tab) = record.iter().position(|byte| *byte == b'\t') else {
        return Err(materialization_error(format!(
            "malformed git ls-tree output for {context}"
        )));
    };
    let path = match std::str::from_utf8(&record[tab + 1..]) {
        Ok(path) => path,
        // The corpus walker excludes non-UTF-8 names and does not descend
        // through a non-UTF-8 component, so callers discard this sentinel.
        Err(_) => {
            return Ok((
                Vec::new(),
                TreeEntry {
                    mode: String::new(),
                    kind: String::new(),
                    object: String::new(),
                    size: None,
                },
            ))
        }
    };
    let header = std::str::from_utf8(&record[..tab]).map_err(|_| {
        materialization_error(format!("malformed git ls-tree output for {context}"))
    })?;
    let fields: Vec<&str> = header.split_ascii_whitespace().collect();
    if fields.len() != 3 && fields.len() != 4 {
        return Err(materialization_error(format!(
            "malformed git ls-tree output for {context}"
        )));
    }
    let components: Vec<String> = path.split('/').map(str::to_string).collect();
    for component in &components {
        validate_portable_component(component, context)?;
    }
    Ok((
        components,
        TreeEntry {
            mode: fields[0].to_string(),
            kind: fields[1].to_string(),
            object: fields[2].to_string(),
            size: fields
                .get(3)
                .filter(|size| **size != "-")
                .and_then(|size| size.parse().ok()),
        },
    ))
}

const MAX_LS_TREE_RECORD_BYTES: usize = V2_MAX_PATH_BYTES + 256;
const MAX_GIT_STDERR_BYTES: u64 = 64 * 1024;

fn drain_git_stderr(mut stderr: impl Read) -> io::Result<Vec<u8>> {
    let mut retained = Vec::new();
    let mut buffer = [0u8; 8 * 1024];
    loop {
        let read = stderr.read(&mut buffer)?;
        if read == 0 {
            return Ok(retained);
        }
        let remaining = (MAX_GIT_STDERR_BYTES as usize).saturating_sub(retained.len());
        retained.extend_from_slice(&buffer[..read.min(remaining)]);
    }
}

fn read_nul_record(
    reader: &mut impl BufRead,
    context: &str,
) -> Result<Option<Vec<u8>>, RevisionSnapshotError> {
    let mut record = Vec::new();
    loop {
        let buffer = reader.fill_buf().map_err(|error| {
            materialization_error(format!("cannot read git ls-tree output for {context}: {error}"))
        })?;
        if buffer.is_empty() {
            if record.is_empty() {
                return Ok(None);
            }
            return Err(materialization_error(format!(
                "unterminated git ls-tree record for {context}"
            )));
        }

        let delimiter = buffer.iter().position(|byte| *byte == 0);
        let take = delimiter.unwrap_or(buffer.len());
        if record.len().saturating_add(take) > MAX_LS_TREE_RECORD_BYTES {
            return Err(materialization_error(format!(
                "git ls-tree record exceeds bounded path size for {context}"
            )));
        }
        record.extend_from_slice(&buffer[..take]);
        reader.consume(take + usize::from(delimiter.is_some()));
        if delimiter.is_some() {
            return Ok(Some(record));
        }
    }
}

fn visit_recursive_tree_records(
    repository: &Path,
    commit: &str,
    selected: &[String],
    visit: impl FnMut(&[u8]) -> Result<(), RevisionSnapshotError>,
) -> Result<(), RevisionSnapshotError> {
    let selected_path = selected.join("/");
    let command = if selected.is_empty() {
        git_command(&["ls-tree", "-r", "-t", "-l", "-z", commit], repository)
    } else {
        let literal = format!(":(literal){selected_path}");
        git_command(
            &["ls-tree", "-r", "-t", "-l", "-z", commit, "--", &literal],
            repository,
        )
    };
    let context = if selected.is_empty() {
        "."
    } else {
        &selected_path
    };
    visit_tree_command(command, context, visit)
}

fn visit_tree_command(
    mut command: Command,
    context: &str,
    mut visit: impl FnMut(&[u8]) -> Result<(), RevisionSnapshotError>,
) -> Result<(), RevisionSnapshotError> {
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|_| {
            RevisionSnapshotError::NotAGitRepository("git executable not found".to_string())
        })?;
    let stdout = child.stdout.take().ok_or_else(|| {
        materialization_error("git ls-tree stdout unavailable".to_string())
    })?;
    let stderr = child.stderr.take().ok_or_else(|| {
        materialization_error("git ls-tree stderr unavailable".to_string())
    })?;
    let stderr_reader = std::thread::spawn(move || drain_git_stderr(stderr));

    let mut stdout = BufReader::new(stdout);
    let visit_result = (|| -> Result<(), RevisionSnapshotError> {
        while let Some(record) = read_nul_record(&mut stdout, context)? {
            if !record.is_empty() {
                visit(&record)?;
            }
        }
        Ok(())
    })();
    if visit_result.is_err() {
        let _ = child.kill();
    }
    drop(stdout);
    let status = child
        .wait()
        .map_err(|error| materialization_error(format!("cannot wait for git ls-tree: {error}")))?;
    let stderr = stderr_reader
        .join()
        .map_err(|_| materialization_error("cannot join git ls-tree stderr reader".to_string()))?;
    let stderr = stderr.map_err(|error| {
        materialization_error(format!("cannot read git ls-tree stderr: {error}"))
    })?;
    visit_result?;
    if !status.success() {
        let stderr = String::from_utf8_lossy(&stderr);
        return Err(materialization_error(format!(
            "git ls-tree failed for {}: {}",
            context,
            if stderr.trim().is_empty() {
                status.to_string()
            } else {
                stderr.trim().to_string()
            }
        )));
    }
    Ok(())
}

fn visit_tree_children(
    repository: &Path,
    commit: &str,
    directory: &[String],
    visit: impl FnMut(&[u8]) -> Result<(), RevisionSnapshotError>,
) -> Result<(), RevisionSnapshotError> {
    let directory_path = directory.join("/");
    let command = if directory.is_empty() {
        git_command(&["ls-tree", "-t", "-l", "-z", commit], repository)
    } else {
        let literal = format!(":(literal){directory_path}/");
        git_command(
            &["ls-tree", "-t", "-l", "-z", commit, "--", &literal],
            repository,
        )
    };
    visit_tree_command(
        command,
        if directory.is_empty() {
            "."
        } else {
            &directory_path
        },
        visit,
    )
}

#[allow(clippy::too_many_arguments)]
fn collect_included_frontier(
    repository: &Path,
    commit: &str,
    directory: &[String],
    selected_root: &[String],
    global_prefix: &[String],
    excluded: &[Vec<String>],
    limits: &mut CorpusLimits,
    traversed: &mut usize,
    path_bytes: &mut usize,
    included: &mut Vec<Vec<String>>,
) -> Result<(), RevisionSnapshotError> {
    let context = if directory.is_empty() {
        ".".to_string()
    } else {
        directory.join("/")
    };
    visit_tree_children(repository, commit, directory, |record| {
        limits.charge_query(&context)?;
        let (path, entry) = parse_tree_record(record, &context)?;
        if path.is_empty() {
            limits.charge_unkeyed_streamed(&context)?;
            return Ok(());
        }
        if let Some(relative) = strip_component_prefix(&path, selected_root) {
            let global_path: Vec<String> = global_prefix
                .iter()
                .cloned()
                .chain(relative.iter().cloned())
                .collect();
            let display_path = if global_path.is_empty() {
                ".".to_string()
            } else {
                global_path.join("/")
            };
            limits.charge_streamed(&display_path, &entry)?;
        }
        if path == directory || directory.starts_with(&path) {
            return Ok(());
        }
        let Some(relative) = strip_component_prefix(&path, directory) else {
            return Err(materialization_error(format!(
                "git ls-tree returned path {} outside frontier directory {context}",
                path.join("/")
            )));
        };
        if relative.len() != 1 {
            return Err(materialization_error(format!(
                "git ls-tree recursively entered {} while pruning {context}",
                path.join("/")
            )));
        }
        *traversed = traversed.saturating_add(1);
        if *traversed > V2_MAX_VISITED_ENTRIES {
            return Err(materialization_error(format!(
                "revision snapshot exclusion frontier exceeds entry limit {V2_MAX_VISITED_ENTRIES}"
            )));
        }
        if excluded.iter().any(|root| root == &path) {
            return Ok(());
        }
        if excluded
            .iter()
            .any(|root| root.len() > path.len() && root.starts_with(&path))
        {
            if entry.kind == "commit" && entry.mode == "160000" {
                let bytes = path.join("/").len();
                *path_bytes = path_bytes.checked_add(bytes).ok_or_else(|| {
                    materialization_error(
                        "revision snapshot frontier byte count overflow".to_string(),
                    )
                })?;
                if *path_bytes > V2_MAX_PHYSICAL_BYTES {
                    return Err(materialization_error(format!(
                        "revision snapshot exclusion frontier exceeds {V2_MAX_PHYSICAL_BYTES} path bytes"
                    )));
                }
                // The superproject walk sees only the gitlink. Its normal
                // processing follows the recorded local ODB, where the
                // descendant exclusion is converted to submodule-local form
                // and physically pruned by the recursive corpus scan.
                included.push(path);
                return Ok(());
            }
            if entry.kind != "tree" {
                return Err(materialization_error(format!(
                    "revision snapshot exclusion crosses non-tree path {}",
                    path.join("/")
                )));
            }
            return collect_included_frontier(
                repository,
                commit,
                &path,
                selected_root,
                global_prefix,
                excluded,
                limits,
                traversed,
                path_bytes,
                included,
            );
        }
        let bytes = path.join("/").len();
        *path_bytes = path_bytes.checked_add(bytes).ok_or_else(|| {
            materialization_error("revision snapshot frontier byte count overflow".to_string())
        })?;
        if *path_bytes > V2_MAX_PHYSICAL_BYTES {
            return Err(materialization_error(format!(
                "revision snapshot exclusion frontier exceeds {V2_MAX_PHYSICAL_BYTES} path bytes"
            )));
        }
        included.push(path);
        Ok(())
    })
}

fn visit_frontier_tree_records(
    repository: &Path,
    commit: &str,
    selected: &[String],
    included: &[Vec<String>],
    mut visit: impl FnMut(&[u8]) -> Result<(), RevisionSnapshotError>,
) -> Result<(), RevisionSnapshotError> {
    const MAX_PATHSPEC_BATCH_BYTES: usize = 32 * 1024;
    const MAX_PATHSPEC_BATCH_COUNT: usize = 256;

    let context = if selected.is_empty() {
        ".".to_string()
    } else {
        selected.join("/")
    };
    let mut start = 0usize;
    while start < included.len() {
        let mut command = base_git_command(repository);
        command.args(["ls-tree", "-r", "-t", "-l", "-z", commit, "--"]);
        let mut bytes = 0usize;
        let mut end = start;
        while end < included.len() && end - start < MAX_PATHSPEC_BATCH_COUNT {
            let path = included[end].join("/");
            let next_bytes = bytes.saturating_add(path.len() + 16);
            if end > start && next_bytes > MAX_PATHSPEC_BATCH_BYTES {
                break;
            }
            command.arg(format!(":(literal){path}"));
            bytes = next_bytes;
            end += 1;
        }
        visit_tree_command(command, &context, |record| visit(record))?;
        start = end;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn collect_corpus_tasks(
    repository: &Path,
    commit: &str,
    selected: &[String],
    global_prefix: &[String],
    snapshot_root: &Path,
    submodule_depth: usize,
    limits: &mut CorpusLimits,
    batches: &mut BTreeMap<(PathBuf, String), Vec<BlobTask>>,
    directories: &mut Vec<PathBuf>,
    excluded_roots: &[Vec<String>],
    symlink_policy: CorpusSymlinkPolicy,
) -> Result<(), RevisionSnapshotError> {
    if submodule_depth > V2_MAX_INHERITANCE_DEPTH {
        return Err(materialization_error(format!(
            "revision snapshot exceeds nested submodule depth limit {V2_MAX_INHERITANCE_DEPTH}"
        )));
    }
    let selected_path = selected.join("/");
    let context = if selected.is_empty() {
        "."
    } else {
        &selected_path
    };
    let mut local_exclusions = Vec::new();
    for excluded in excluded_roots {
        if excluded.len() > global_prefix.len() && excluded.starts_with(global_prefix) {
            let mut local = selected.to_vec();
            local.extend_from_slice(&excluded[global_prefix.len()..]);
            local_exclusions.push(local);
        }
    }
    local_exclusions.sort();
    local_exclusions.dedup();

    let included = if local_exclusions.is_empty() {
        None
    } else {
        let mut included = Vec::new();
        let mut traversed = 0usize;
        let mut path_bytes = 0usize;
        collect_included_frontier(
            repository,
            commit,
            selected,
            selected,
            global_prefix,
            &local_exclusions,
            limits,
            &mut traversed,
            &mut path_bytes,
            &mut included,
        )?;
        Some(included)
    };

    let mut visit_record = |record: &[u8]| {
        limits.charge_query(context)?;
        let (local_path, entry) = parse_tree_record(record, context)?;
        if local_path.is_empty() {
            limits.charge_unkeyed_streamed(context)?;
            return Ok(());
        }
        let Some(relative) = strip_component_prefix(&local_path, selected) else {
            // `ls-tree -t` reports the selected path's ancestor tree entries
            // before the selected subtree. They are routing metadata, not
            // corpus entries.
            if selected.starts_with(&local_path) {
                return Ok(());
            }
            return Err(materialization_error(format!(
                "git ls-tree returned path {} outside selected corpus {}",
                local_path.join("/"),
                selected.join("/")
            )));
        };
        if relative.is_empty() {
            return Ok(());
        }
        let global_path: Vec<String> = global_prefix
            .iter()
            .cloned()
            .chain(relative.iter().cloned())
            .collect();
        validate_snapshot_path(&global_path)?;
        let display_path = global_path.join("/");
        limits.charge_streamed(&display_path, &entry)?;
        if excluded_roots
            .iter()
            .any(|excluded| global_path.starts_with(excluded))
        {
            return Ok(());
        }
        let hidden = relative.iter().any(|component| component.starts_with('.'));
        let markdown_named = relative.last().is_some_and(|name| name.ends_with(".md"));
        if entry.mode == "120000"
            && (symlink_policy == CorpusSymlinkPolicy::RejectAll
                || (!hidden && markdown_named))
        {
            return Err(materialization_error(format!(
                "committed symlink is unsupported in revision snapshot corpus: {display_path}"
            )));
        }
        if hidden {
            return Ok(());
        }
        if !limits.admit(&display_path, &entry)? {
            return Ok(());
        }

        match entry.kind.as_str() {
            "commit" => {
                let local_submodule = require_local_submodule(
                    repository,
                    commit,
                    &local_path,
                    &entry.object,
                    &display_path,
                )?;
                if markdown_named {
                    directories.push(join_components(snapshot_root, &global_path));
                }
                collect_corpus_tasks(
                    &local_submodule,
                    &entry.object,
                    &[],
                    &global_path,
                    snapshot_root,
                    submodule_depth + 1,
                    limits,
                    batches,
                    directories,
                    excluded_roots,
                    symlink_policy,
                )?;
            }
            "tree" if markdown_named => {
                directories.push(join_components(snapshot_root, &global_path));
            }
            "tree" => {}
            "blob" if markdown_named => {
                if entry.mode == "120000" {
                    return Err(materialization_error(format!(
                        "committed symlink Markdown is unsupported in revision snapshots: {}",
                        global_path.join("/")
                    )));
                }
                let size = entry.size.ok_or_else(|| {
                    materialization_error(format!(
                        "git ls-tree did not report a blob size for {}",
                        global_path.join("/")
                    ))
                })?;
                if size > V2_MAX_FILE_BYTES as u64 {
                    return Err(materialization_error(format!(
                        "revision snapshot file exceeds {V2_MAX_FILE_BYTES} bytes: {}",
                        global_path.join("/")
                    )));
                }
                limits.files += 1;
                if limits.files > V2_MAX_INHERITED_FILES {
                    return Err(materialization_error(format!(
                        "revision snapshot exceeds Markdown file limit {V2_MAX_INHERITED_FILES}"
                    )));
                }
                limits.physical_bytes =
                    limits.physical_bytes.checked_add(size).ok_or_else(|| {
                        materialization_error("revision snapshot byte count overflow".into())
                    })?;
                if limits.physical_bytes > V2_MAX_PHYSICAL_BYTES as u64 {
                    return Err(materialization_error(format!(
                        "revision snapshot exceeds physical byte limit {V2_MAX_PHYSICAL_BYTES}"
                    )));
                }
                batches
                    .entry((repository.to_path_buf(), commit.to_string()))
                    .or_default()
                    .push(BlobTask {
                        entry,
                        destination: join_components(snapshot_root, &global_path),
                        display_path,
                    });
            }
            "blob" => {}
            _ => {
                return Err(materialization_error(format!(
                    "unsupported Git tree entry for corpus path {}",
                    global_path.join("/")
                )));
            }
        }
        Ok(())
    };

    if let Some(included) = included.as_deref() {
        visit_frontier_tree_records(repository, commit, selected, included, &mut visit_record)
    } else {
        visit_recursive_tree_records(repository, commit, selected, &mut visit_record)
    }
}

fn validate_snapshot_path(components: &[String]) -> Result<(), RevisionSnapshotError> {
    if components.len() > V2_MAX_PATH_COMPONENTS {
        return Err(materialization_error(format!(
            "revision snapshot path exceeds {V2_MAX_PATH_COMPONENTS} components"
        )));
    }
    let path = components.join("/");
    for component in components {
        validate_portable_component(component, &path)?;
    }
    if path.len() > V2_MAX_PATH_BYTES {
        return Err(materialization_error(format!(
            "revision snapshot path exceeds {V2_MAX_PATH_BYTES} UTF-8 bytes: {path}"
        )));
    }
    Ok(())
}

fn materialize_blob_batch(
    repository: &Path,
    tasks: &[BlobTask],
) -> Result<(), RevisionSnapshotError> {
    if tasks.is_empty() {
        return Ok(());
    }
    let mut command = git_command(&["cat-file", "--batch"], repository);
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|_| {
            RevisionSnapshotError::NotAGitRepository("git executable not found".to_string())
        })?;
    let stderr = child.stderr.take().ok_or_else(|| {
        materialization_error("git cat-file stderr unavailable".to_string())
    })?;
    let stderr_reader = std::thread::spawn(move || drain_git_stderr(stderr));

    let exchange_result =
        (|| -> Result<(), RevisionSnapshotError> {
            let mut stdin = child.stdin.take().ok_or_else(|| {
                materialization_error("git cat-file stdin unavailable".to_string())
            })?;
            let stdout = child.stdout.take().ok_or_else(|| {
                materialization_error("git cat-file stdout unavailable".to_string())
            })?;
            let mut stdout = BufReader::new(stdout);
            for task in tasks {
                stdin
                    .write_all(task.entry.object.as_bytes())
                    .and_then(|()| stdin.write_all(b"\n"))
                    .and_then(|()| stdin.flush())
                    .map_err(|error| {
                        materialization_error(format!(
                            "cannot request committed blob for {}: {error}",
                            task.display_path
                        ))
                    })?;
                read_blob_response(&mut stdout, task)?;
            }
            Ok(())
        })();

    if exchange_result.is_err() {
        let _ = child.kill();
    }

    let status = child
        .wait()
        .map_err(|error| materialization_error(format!("cannot wait for git cat-file: {error}")))?;
    let stderr = stderr_reader.join().map_err(|_| {
        materialization_error("cannot join git cat-file stderr reader".to_string())
    })?;
    let stderr = stderr.map_err(|error| {
        materialization_error(format!("cannot read git cat-file stderr: {error}"))
    })?;
    exchange_result?;
    if !status.success() {
        let stderr = String::from_utf8_lossy(&stderr);
        return Err(materialization_error(format!(
            "git cat-file --batch failed: {}",
            if stderr.trim().is_empty() {
                status.to_string()
            } else {
                stderr.trim().to_string()
            }
        )));
    }
    Ok(())
}

fn read_blob_response(
    stdout: &mut impl BufRead,
    task: &BlobTask,
) -> Result<(), RevisionSnapshotError> {
    let mut header = String::new();
    stdout.read_line(&mut header).map_err(|error| {
        materialization_error(format!(
            "cannot read committed blob header for {}: {error}",
            task.display_path
        ))
    })?;
    let fields: Vec<&str> = header
        .trim_end_matches('\n')
        .split_ascii_whitespace()
        .collect();
    if fields.len() != 3 || fields[0] != task.entry.object || fields[1] != "blob" {
        return Err(materialization_error(format!(
            "git cat-file returned an invalid blob header for {}",
            task.display_path
        )));
    }
    let size: usize = fields[2].parse().map_err(|_| {
        materialization_error(format!(
            "git cat-file returned an invalid blob size for {}",
            task.display_path
        ))
    })?;
    if task.entry.size != Some(size as u64) || size > V2_MAX_FILE_BYTES {
        return Err(materialization_error(format!(
            "committed blob size changed while materializing {}",
            task.display_path
        )));
    }
    let mut bytes = vec![0; size];
    stdout.read_exact(&mut bytes).map_err(|error| {
        materialization_error(format!(
            "cannot read committed blob for {}: {error}",
            task.display_path
        ))
    })?;
    let mut delimiter = [0u8; 1];
    stdout.read_exact(&mut delimiter).map_err(|error| {
        materialization_error(format!(
            "cannot read blob delimiter for {}: {error}",
            task.display_path
        ))
    })?;
    if delimiter[0] != b'\n' {
        return Err(materialization_error(format!(
            "git cat-file returned a malformed blob for {}",
            task.display_path
        )));
    }
    write_regular_blob(&task.entry, &bytes, &task.destination, &task.display_path)?;
    Ok(())
}

fn materialize_blob(
    repository: &Path,
    entry: &TreeEntry,
    destination: &Path,
    display_path: &str,
) -> Result<(), RevisionSnapshotError> {
    if entry.mode == "120000" {
        return Err(materialization_error(format!(
            "committed symlink is unsupported in revision snapshots: {display_path}"
        )));
    }
    let blob = run_git(&["cat-file", "blob", &entry.object], repository)?;
    if !blob.status.success() {
        return Err(materialization_error(format!(
            "cannot read committed blob for {display_path}: {}",
            command_stderr(&blob)
        )));
    }
    if entry.size != Some(blob.stdout.len() as u64) || blob.stdout.len() > V2_MAX_FILE_BYTES {
        return Err(materialization_error(format!(
            "committed blob size changed while materializing {display_path}"
        )));
    }
    write_regular_blob(entry, &blob.stdout, destination, display_path)
}

fn write_regular_blob(
    entry: &TreeEntry,
    bytes: &[u8],
    destination: &Path,
    display_path: &str,
) -> Result<(), RevisionSnapshotError> {
    if entry.mode != "100644" && entry.mode != "100755" {
        return Err(materialization_error(format!(
            "unsupported Git blob mode {} for {display_path}",
            entry.mode
        )));
    }
    if let Some(parent) = destination.parent() {
        std::fs::create_dir_all(parent).map_err(|error| {
            materialization_error(format!(
                "cannot create snapshot directory for {display_path}: {error}"
            ))
        })?;
    }
    if std::fs::symlink_metadata(destination)
        .is_ok_and(|metadata| metadata.file_type().is_symlink())
    {
        std::fs::remove_file(destination).map_err(|error| {
            materialization_error(format!(
                "cannot replace snapshot symlink {display_path}: {error}"
            ))
        })?;
    }
    std::fs::write(destination, bytes).map_err(|error| {
        materialization_error(format!(
            "cannot write snapshot file {display_path}: {error}"
        ))
    })?;
    #[cfg(unix)]
    if entry.mode == "100755" {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(destination, std::fs::Permissions::from_mode(0o755)).map_err(
            |error| {
                materialization_error(format!(
                    "cannot set snapshot permissions for {display_path}: {error}"
                ))
            },
        )?;
    }
    Ok(())
}

fn tree_entry(
    repository: &Path,
    commit: &str,
    path: &str,
) -> Result<Option<TreeEntry>, RevisionSnapshotError> {
    let literal_pathspec = format!(":(literal){path}");
    let output = run_git(
        &["ls-tree", "-l", "-z", commit, "--", &literal_pathspec],
        repository,
    )?;
    if !output.status.success() {
        return Err(materialization_error(format!(
            "git ls-tree failed for {path}: {}",
            command_stderr(&output)
        )));
    }
    if output.stdout.is_empty() {
        return Ok(None);
    }
    let mut records = output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|r| !r.is_empty());
    let Some(record) = records.next() else {
        return Ok(None);
    };
    if records.next().is_some() {
        return Err(materialization_error(format!(
            "git ls-tree returned multiple entries for literal path {path}"
        )));
    }
    let Some(tab) = record.iter().position(|byte| *byte == b'\t') else {
        return Err(materialization_error(format!(
            "malformed git ls-tree output for {path}"
        )));
    };
    if &record[tab + 1..] != path.as_bytes() {
        return Err(materialization_error(format!(
            "git ls-tree returned an unexpected path for {path}"
        )));
    }
    let header = std::str::from_utf8(&record[..tab])
        .map_err(|_| materialization_error(format!("malformed git ls-tree output for {path}")))?;
    let fields: Vec<&str> = header.split_ascii_whitespace().collect();
    if fields.len() != 4 {
        return Err(materialization_error(format!(
            "malformed git ls-tree output for {path}"
        )));
    }
    Ok(Some(TreeEntry {
        mode: fields[0].to_string(),
        kind: fields[1].to_string(),
        object: fields[2].to_string(),
        size: if fields[3] == "-" {
            None
        } else {
            Some(fields[3].parse().map_err(|_| {
                materialization_error(format!("malformed git ls-tree size for {path}"))
            })?)
        },
    }))
}

fn require_local_submodule(
    parent_repository: &Path,
    parent_commit: &str,
    submodule_components: &[String],
    commit: &str,
    repo_relative: &str,
) -> Result<PathBuf, RevisionSnapshotError> {
    let parent_git_dir = absolute_git_common_dir(parent_repository)?.ok_or_else(|| {
        materialization_error(format!(
            "cannot locate parent Git object database for submodule: {repo_relative}"
        ))
    })?;
    let module_components = submodule_name_for_path(
        parent_repository,
        parent_commit,
        &submodule_components.join("/"),
    )?
    .unwrap_or_else(|| submodule_components.to_vec());
    let module_candidate = join_components(&parent_git_dir.join("modules"), &module_components);
    let mut found_repository = false;

    if let Some(git_dir) = absolute_git_dir(&module_candidate)? {
        found_repository = true;
        if commit_is_available(&git_dir, commit)? {
            return Ok(git_dir);
        }
    }

    let worktree = run_git(&["rev-parse", "--show-toplevel"], parent_repository)?;
    if worktree.status.success() {
        let worktree = std::str::from_utf8(&worktree.stdout)
            .map_err(|_| materialization_error("Git work-tree path is not UTF-8".to_string()))?
            .trim();
        if !worktree.is_empty() {
            let checkout = join_components(Path::new(worktree), submodule_components);
            if let Some(git_dir) = absolute_git_dir(&checkout)? {
                found_repository = true;
                if commit_is_available(&git_dir, commit)? {
                    return Ok(git_dir);
                }
            }
        }
    }

    if found_repository {
        return Err(materialization_error(format!(
            "recorded submodule commit is not available locally: {repo_relative} at {commit}"
        )));
    }
    Err(materialization_error(format!(
        "submodule object database is not available locally: {repo_relative}"
    )))
}

fn submodule_name_for_path(
    repository: &Path,
    commit: &str,
    submodule_path: &str,
) -> Result<Option<Vec<String>>, RevisionSnapshotError> {
    let Some(attributes) = tree_entry(repository, commit, ".gitmodules")? else {
        return Ok(None);
    };
    if attributes.kind != "blob" || attributes.mode == "120000" {
        return Err(materialization_error(
            "committed .gitmodules must be a regular file".to_string(),
        ));
    }
    let size = attributes.size.ok_or_else(|| {
        materialization_error("git ls-tree did not report a .gitmodules size".to_string())
    })?;
    if size > V2_MAX_FILE_BYTES as u64 {
        return Err(materialization_error(format!(
            "committed .gitmodules exceeds {V2_MAX_FILE_BYTES} bytes"
        )));
    }

    let blob = format!("--blob={commit}:.gitmodules");
    let output = run_git(
        &[
            "config",
            "--null",
            "--no-includes",
            &blob,
            "--get-regexp",
            "^submodule\\..*\\.path$",
        ],
        repository,
    )?;
    if !output.status.success() {
        if output.status.code() == Some(1) {
            return Ok(None);
        }
        return Err(materialization_error(format!(
            "cannot read committed .gitmodules: {}",
            command_stderr(&output)
        )));
    }
    if output.stdout.len() > V2_MAX_FILE_BYTES {
        return Err(materialization_error(format!(
            "committed .gitmodules query exceeds {V2_MAX_FILE_BYTES} bytes"
        )));
    }

    let mut matched: Option<Vec<String>> = None;
    for record in output.stdout.split(|byte| *byte == 0) {
        if record.is_empty() {
            continue;
        }
        let record = std::str::from_utf8(record).map_err(|_| {
            materialization_error("committed .gitmodules is not UTF-8".to_string())
        })?;
        let Some((key, value)) = record.split_once('\n') else {
            return Err(materialization_error(
                "malformed committed .gitmodules query result".to_string(),
            ));
        };
        if value != submodule_path {
            continue;
        }
        let Some(name) = key
            .strip_prefix("submodule.")
            .and_then(|key| key.strip_suffix(".path"))
        else {
            return Err(materialization_error(
                "malformed committed .gitmodules key".to_string(),
            ));
        };
        let components: Vec<String> = name.split('/').map(str::to_string).collect();
        validate_snapshot_path(&components)?;
        if matched.as_ref().is_some_and(|previous| previous != &components) {
            return Err(materialization_error(format!(
                "multiple committed submodule names select path {submodule_path}"
            )));
        }
        matched = Some(components);
    }
    Ok(matched)
}

fn absolute_git_dir(repository: &Path) -> Result<Option<PathBuf>, RevisionSnapshotError> {
    if !repository.is_dir() {
        return Ok(None);
    }
    let output = run_git(&["rev-parse", "--absolute-git-dir"], repository)?;
    if !output.status.success() {
        return Ok(None);
    }
    let git_dir = std::str::from_utf8(&output.stdout)
        .map_err(|_| materialization_error("Git object database path is not UTF-8".to_string()))?
        .trim();
    if git_dir.is_empty() {
        return Ok(None);
    }
    let git_dir = std::fs::canonicalize(git_dir).map_err(|error| {
        materialization_error(format!("cannot resolve local Git object database: {error}"))
    })?;
    Ok(Some(git_dir))
}

fn absolute_git_common_dir(repository: &Path) -> Result<Option<PathBuf>, RevisionSnapshotError> {
    if !repository.is_dir() {
        return Ok(None);
    }
    let output = run_git(&["rev-parse", "--git-common-dir"], repository)?;
    if !output.status.success() {
        return Ok(None);
    }
    let git_dir = std::str::from_utf8(&output.stdout)
        .map_err(|_| materialization_error("Git common directory is not UTF-8".to_string()))?
        .trim();
    if git_dir.is_empty() {
        return Ok(None);
    }
    let git_dir = Path::new(git_dir);
    let git_dir = if git_dir.is_absolute() {
        git_dir.to_path_buf()
    } else {
        repository.join(git_dir)
    };
    let git_dir = std::fs::canonicalize(git_dir).map_err(|error| {
        materialization_error(format!("cannot resolve local Git common directory: {error}"))
    })?;
    Ok(Some(git_dir))
}

fn commit_is_available(repository: &Path, commit: &str) -> Result<bool, RevisionSnapshotError> {
    let object = run_git(
        &["cat-file", "-e", &format!("{commit}^{{commit}}")],
        repository,
    )?;
    Ok(object.status.success())
}

/// `materialized_revision(repo_root, rev, subpath)` — verify the commit,
/// archive the subpath, extract into a temp tree, and yield `tmp/<subpath>`
/// (created empty when the archive had nothing to say).
pub fn materialize_revision(
    repo_root: &str,
    rev: &str,
    subpath: &str,
) -> Result<MaterializedRevision, RevisionError> {
    let root = Path::new(repo_root);
    let verify = run_git(&["rev-parse", "--verify", "--quiet", &format!("{rev}^{{commit}}")], root)?;
    if !verify.status.success() {
        return Err(RevisionError::RevisionNotFound(format!(
            "unknown revision: {rev}"
        )));
    }

    let pathspec = if subpath.is_empty() || subpath == "." {
        "."
    } else {
        subpath
    };
    let archive = run_git(&["archive", "--format=tar", rev, "--", pathspec], root)?;

    let tmp = make_temp_dir("decided-watchkeeper").map_err(|e| {
        // No oracle-comparable surface exists for a failing temp root; the
        // closest degrade is the not-a-repository class (never hit by the
        // parity fixtures).
        RevisionError::NotAGitRepository(format!("not a git repository: {e}"))
    })?;
    let guard_root = tmp.clone();
    if archive.status.success() {
        extract_tar(&archive.stdout, &tmp);
    }
    // A nonzero archive exit means the subpath does not exist at `rev`:
    // materialize an empty corpus rather than failing the comparison.
    let corpus = if pathspec == "." {
        tmp
    } else {
        guard_root.join(subpath)
    };
    let _ = std::fs::create_dir_all(&corpus);
    Ok(MaterializedRevision {
        root: guard_root,
        corpus,
    })
}

// ---------------------------------------------------------------------------
// Minimal tar reader — enough for `git archive --format=tar` output: ustar
// headers with the split name/prefix fields, the pax global header git
// always emits ('g', skipped), pax extended headers ('x', `path=` override),
// GNU longname ('L'), directories ('5'), regular files ('0'/NUL), and
// symlinks ('2', created best-effort). Entries with absolute or `..`
// components are skipped defensively (tarfile's `filter="data"` would raise
// there; git archive never produces them).
// ---------------------------------------------------------------------------

fn octal_field(bytes: &[u8]) -> u64 {
    let mut out: u64 = 0;
    for &b in bytes {
        if matches!(b, b'0'..=b'7') {
            out = out * 8 + u64::from(b - b'0');
        }
    }
    out
}

fn cstr_field(bytes: &[u8]) -> String {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..end]).into_owned()
}

/// Parse a pax extended-header payload (`<len> <key>=<value>\n` records)
/// and return the `path` override, if any.
fn pax_path(data: &[u8]) -> Option<String> {
    let mut i = 0;
    while i < data.len() {
        // "<decimal-length> <key>=<value>\n" — length covers the whole record.
        let space = data[i..].iter().position(|&b| b == b' ')?;
        let len: usize = std::str::from_utf8(&data[i..i + space])
            .ok()?
            .parse()
            .ok()?;
        if len == 0 || i + len > data.len() {
            return None;
        }
        let record = &data[i + space + 1..i + len];
        if let Some(eq) = record.iter().position(|&b| b == b'=') {
            let key = &record[..eq];
            if key == b"path" {
                let mut value = &record[eq + 1..];
                if value.last() == Some(&b'\n') {
                    value = &value[..value.len() - 1];
                }
                return Some(String::from_utf8_lossy(value).into_owned());
            }
        }
        i += len;
    }
    None
}

/// True when every component of the '/'-separated relative path is safe to
/// join under the extraction root.
fn safe_relative(name: &str) -> bool {
    !name.starts_with('/')
        && !name
            .split('/')
            .any(|c| c == ".." || c.chars().any(|ch| ch == '\\'))
}

/// Extract a tar stream under `target`. Unknown/unsafe entries are skipped;
/// extraction is best-effort (the oracle's crash surfaces here are out of
/// the refereed contract — git-produced archives are always well-formed).
fn extract_tar(data: &[u8], target: &Path) {
    let mut offset = 0usize;
    let mut pending_path: Option<String> = None;
    while offset + 512 <= data.len() {
        let header = &data[offset..offset + 512];
        offset += 512;
        if header.iter().all(|&b| b == 0) {
            break; // end-of-archive zero block
        }
        let size = octal_field(&header[124..136]) as usize;
        let padded = size.div_ceil(512) * 512;
        if offset + size > data.len() {
            break; // truncated
        }
        let body = &data[offset..offset + size];
        let typeflag = header[156];
        match typeflag {
            b'g' => {} // pax global header (git's comment=<sha>) — skip
            b'x' => {
                if let Some(p) = pax_path(body) {
                    pending_path = Some(p);
                }
            }
            b'L' => {
                // GNU longname: NUL-terminated name for the next entry.
                pending_path = Some(cstr_field(body));
            }
            _ => {
                let mut name = match pending_path.take() {
                    Some(p) => p,
                    None => {
                        let base = cstr_field(&header[0..100]);
                        let prefix = cstr_field(&header[345..500]);
                        if prefix.is_empty() {
                            base
                        } else {
                            format!("{prefix}/{base}")
                        }
                    }
                };
                let is_dir_name = name.ends_with('/');
                while name.ends_with('/') {
                    name.pop();
                }
                if !name.is_empty() && safe_relative(&name) {
                    let dest = target.join(&name);
                    match typeflag {
                        b'5' => {
                            let _ = std::fs::create_dir_all(&dest);
                        }
                        b'0' | 0 | b'7' if !is_dir_name => {
                            if let Some(parent) = dest.parent() {
                                let _ = std::fs::create_dir_all(parent);
                            }
                            let _ = std::fs::write(&dest, body);
                        }
                        b'2' => {
                            if let Some(parent) = dest.parent() {
                                let _ = std::fs::create_dir_all(parent);
                            }
                            #[cfg(unix)]
                            {
                                let link = cstr_field(&header[157..257]);
                                let _ = std::os::unix::fs::symlink(&link, &dest);
                            }
                        }
                        _ => {} // hardlinks/devices: never in git archives
                    }
                }
            }
        }
        offset += padded;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn octal_parses_padded_fields() {
        assert_eq!(octal_field(b"0000644\0"), 0o644);
        assert_eq!(octal_field(b"00000000173 "), 0o173);
    }

    #[test]
    fn pax_path_record() {
        let payload = b"33 path=decisions/some-long-name\n";
        assert_eq!(pax_path(payload).as_deref(), Some("decisions/some-long-name"));
    }

    #[test]
    fn rejects_escaping_names() {
        assert!(!safe_relative("/abs"));
        assert!(!safe_relative("a/../b"));
        assert!(safe_relative("decisions/d1.md"));
    }
}
