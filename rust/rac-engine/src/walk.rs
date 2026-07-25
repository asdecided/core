//! Corpus file discovery — a byte-exact port of `find_markdown_files`
//! (`src/rac/core/fs.py`) and the walk seam, per PORT-CONTRACT.d/09 §1.
//!
//! Landmines reproduced here:
//! - Extension filter is the literal glob `*.md`, **case-sensitive** on Linux:
//!   `upper.MD`, `x.Md`, `x.markdown` do not match.
//! - Hidden exclusion: any component of the path **relative to root** that
//!   starts with `.` drops the path (hidden dirs at any depth, and hidden
//!   files). Equivalent to pruning hidden entries during the walk.
//! - Symlink asymmetry (Python 3.11 `rglob`): a symlinked **file** matching
//!   `*.md` IS yielded; a symlinked **directory** is NOT descended.
//! - Sort is **component-wise** (`PurePath._cparts` tuple), NOT whole-string:
//!   `sub/c.md` sorts before `sub-x.md`. We sort by the tuple of relative
//!   components, each compared by Unicode scalar (== UTF-8 byte order).

use std::path::{Path, PathBuf};
use rayon::prelude::*;

/// One discovered markdown file.
#[derive(Debug, Clone)]
pub struct WalkEntry {
    /// Relative path components (relative to the walk root), in order.
    pub components: Vec<String>,
    /// Absolute path on disk, for actual file access.
    pub abs: PathBuf,
    /// `str(path)` as the oracle emits it downstream: the normalized root arg
    /// prefix (PORT-CONTRACT.d/09 §1.6) joined with the relative components.
    pub display: String,
}

impl WalkEntry {
    /// The relative path, `/`-joined — matches `str(p.relative_to(root))`.
    pub fn rel(&self) -> String {
        self.components.join("/")
    }
}

/// Find `*.md` files under `directory`, dropping any path with a dotted
/// component, in component-wise sorted order. `recursive=false` looks only at
/// direct children (`root.glob` instead of `root.rglob`).
pub fn find_markdown_files(directory: &str, recursive: bool) -> Vec<WalkEntry> {
    let root = Path::new(directory);
    let mut found: Vec<(Vec<String>, PathBuf)> = if recursive {
        collect_root_parallel(root)
    } else {
        let mut found = Vec::new();
        collect(root, &mut Vec::new(), false, &mut found);
        found
    };

    // Component-wise sort: compare the tuple of relative components. Rust's
    // `Vec<String>` Ord is lexicographic, and `String` Ord is UTF-8 byte order
    // which equals Unicode scalar order — exactly Python's `_cparts` compare.
    found.sort_by(|a, b| a.0.cmp(&b.0));

    let prefix = normalize_root(directory);
    found
        .into_iter()
        .map(|(components, abs)| {
            let display = join_display(&prefix, &components);
            WalkEntry {
                components,
                abs,
                display,
            }
        })
        .collect()
}

/// Split a recursive walk across the root's immediate children. Results are
/// sorted after collection, so scheduling cannot affect the oracle-visible
/// component order. Deeper recursion stays serial within each shard, avoiding
/// task creation per directory.
fn collect_root_parallel(root: &Path) -> Vec<(Vec<String>, PathBuf)> {
    let entries = match std::fs::read_dir(root) {
        Ok(entries) => entries,
        Err(_) => return Vec::new(),
    };
    let roots: Vec<(String, PathBuf, bool, bool)> = entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            if name.starts_with('.') {
                return None;
            }
            let file_type = entry.file_type().ok();
            Some((
                name,
                entry.path(),
                file_type.as_ref().is_some_and(|kind| kind.is_dir()),
                file_type.as_ref().is_some_and(|kind| kind.is_symlink()),
            ))
        })
        .collect();
    roots
        .into_par_iter()
        .map(|(name, path, is_dir, is_symlink)| {
            let mut local = Vec::new();
            if name.ends_with(".md") {
                local.push((vec![name.clone()], path.clone()));
            }
            if is_dir && !is_symlink {
                let mut rel = vec![name];
                collect(&path, &mut rel, true, &mut local);
            }
            local
        })
        .flatten()
        .collect()
}

/// Recursive directory walk. `rel` is the component stack from the root to
/// `dir`. Hidden entries (name starting with `.`) are pruned wholesale, which
/// is equivalent to the oracle's post-hoc "any relative part starts with `.`"
/// filter (nothing under a hidden dir would survive it).
fn collect(
    dir: &Path,
    rel: &mut Vec<String>,
    recursive: bool,
    out: &mut Vec<(Vec<String>, PathBuf)>,
) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let name = match entry.file_name().into_string() {
            Ok(n) => n,
            Err(_) => continue, // non-UTF-8 name: out of corpus scope
        };
        if name.starts_with('.') {
            continue; // hidden component -> excluded (dirs and files)
        }
        // `file_type()` on Unix comes from the directory entry (lstat), so a
        // symlink reports `is_symlink()`, NOT `is_dir()` — matching Python
        // 3.11 rglob, which descends only real (non-symlink) directories.
        let ft = entry.file_type();
        let is_symlink = ft.as_ref().map(|t| t.is_symlink()).unwrap_or(false);
        let is_dir = ft.as_ref().map(|t| t.is_dir()).unwrap_or(false);

        // `*.md` name match — case-sensitive. `rglob("*.md")` globs the name
        // regardless of entry type, so symlinked files (and, as an edge, dirs
        // named `*.md`) are yielded.
        if name.ends_with(".md") {
            rel.push(name.clone());
            out.push((rel.clone(), entry.path()));
            rel.pop();
        }

        if recursive && is_dir && !is_symlink {
            rel.push(name);
            collect(&entry.path(), rel, recursive, out);
            rel.pop();
        }
    }
}

/// Build `str(root / rel)` — the normalized root prefix joined to the relative
/// components with `/`. Mirrors `str(path)` for paths from `rglob`.
fn join_display(prefix: &str, components: &[String]) -> String {
    if prefix.is_empty() || prefix == "." {
        // pathlib drops a bare-`.` root when joining: Path('.')/'a.md' ->
        // PosixPath('a.md'), so walked paths under `.` carry no prefix.
        components.join("/")
    } else if prefix == "/" {
        // Absolute root "/": avoid a doubled leading slash.
        format!("/{}", components.join("/"))
    } else if prefix.ends_with('/') {
        // Only a preserved "//" prefix ends with a slash here.
        format!("{}{}", prefix, components.join("/"))
    } else {
        format!("{}/{}", prefix, components.join("/"))
    }
}

/// Normalize a directory argument the way `str(Path(directory))` does
/// (PurePosixPath semantics, PORT-CONTRACT.d/09 §1.6):
/// - trailing slashes stripped (`decisions/` -> `decided`)
/// - leading `./` stripped (`./decisions/` -> `decided`)
/// - repeated slashes collapsed (`decisions//` -> `decided`), interior `.` removed
///   (`decisions/./x` -> `decisions/x`)
/// - `..` preserved; absolute stays absolute
/// - the empty / `.` argument normalizes to `.`
pub fn normalize_root(directory: &str) -> String {
    // Leading-slash handling matches PurePosixPath: exactly two leading
    // slashes are preserved as "//", one or three-plus collapse to "/".
    let leading = directory.chars().take_while(|&c| c == '/').count();
    let root_prefix = match leading {
        0 => "",
        2 => "//",
        _ => "/",
    };

    let parts: Vec<&str> = directory
        .split('/')
        .filter(|p| !p.is_empty() && *p != ".")
        .collect();

    if root_prefix.is_empty() {
        if parts.is_empty() {
            ".".to_string()
        } else {
            parts.join("/")
        }
    } else if root_prefix == "//" {
        format!("//{}", parts.join("/"))
    } else {
        format!("/{}", parts.join("/"))
    }
}

/// `str(Path(root) / a / b / ...)` — the normalized root joined with literal
/// relative components (skill/hook install destination paths). Reuses the
/// walk's `join_display` semantics: a bare-`.` root vanishes, "/" and a
/// preserved "//" prefix avoid doubled slashes.
pub fn py_join(root: &str, components: &[&str]) -> String {
    let owned: Vec<String> = components.iter().map(|c| (*c).to_string()).collect();
    join_display(&normalize_root(root), &owned)
}

/// What a command should do with a positional path argument: validate/inspect
/// and friends dispatch a single file directly, a directory through the walk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WalkTarget {
    /// The argument names one file — process it directly.
    File(PathBuf),
    /// The argument names a directory — walk it.
    Directory(PathBuf),
    /// The argument is neither (missing / special) — the caller raises the
    /// command's own usage error.
    Missing(PathBuf),
}

/// Classify a positional path argument for single-file vs directory dispatch.
pub fn dispatch(path: &str) -> WalkTarget {
    let p = PathBuf::from(path);
    if p.is_file() {
        WalkTarget::File(p)
    } else if p.is_dir() {
        WalkTarget::Directory(p)
    } else {
        WalkTarget::Missing(p)
    }
}

/// True if `path` is a directory (the guard `Path(arg).is_dir()` used by
/// `stats`/`export`/`review` before walking).
pub fn is_directory(path: &str) -> bool {
    Path::new(path).is_dir()
}
