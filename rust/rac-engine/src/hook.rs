//! Bundled git hooks — `decided hook` (PORT-CONTRACT.d/15).
//!
//! Port of `src/rac/core/hooks.py` (registry + resource loading) and
//! `src/asdecided/services/hook.py` (`install_hook`). The packaged `<style>.sh`
//! scripts are embedded verbatim from `rust/decided-engine/assets/hooks/`,
//! vendored byte-identical copies of the Python package files (unit test
//! below); the INSTALLED file is named `<style>` with no extension, under
//! `<dir>/.git/hooks/`, and is made executable (hook brief, landmines 1-2).

use std::path::Path;

use crate::walk::py_join;

/// One bundled git hook: its style (the git hook filename) and description.
pub struct HookSpec {
    pub style: &'static str,
    pub description: &'static str,
}

/// Bundled hooks, registry order. `install` defaults to the first.
pub const BUNDLED_HOOKS: [HookSpec; 2] = [
    HookSpec {
        style: "post-commit",
        description: "Advisory write-cadence nudge after each commit (never blocks).",
    },
    HookSpec {
        style: "pre-commit",
        description: "Validate staged Markdown artifacts before each commit (blocks on errors).",
    },
];

/// `DEFAULT_STYLE` — the first bundled hook.
pub const DEFAULT_STYLE: &str = "post-commit";

/// The embedded hook scripts, index-aligned with [`BUNDLED_HOOKS`].
pub(crate) const HOOK_BYTES: [&[u8]; 2] = [
    include_bytes!("../assets/hooks/post-commit.sh"),
    include_bytes!("../assets/hooks/pre-commit.sh"),
];

/// `available_hooks()` — bundled hook styles, registry order.
pub fn available_hooks() -> Vec<&'static str> {
    BUNDLED_HOOKS.iter().map(|h| h.style).collect()
}

fn hook_bytes(style: &str) -> Option<&'static [u8]> {
    BUNDLED_HOOKS
        .iter()
        .position(|h| h.style == style)
        .map(|i| HOOK_BYTES[i])
}

/// Result of a `decided hook install` run (`InstalledHook`; `bytes_written` is
/// in the oracle's model but absent from its JSON).
pub struct InstalledHook {
    pub style: String,
    pub path: String,
}

/// The failure contract of `install_hook`, message-shaped like the oracle.
/// `HookNotFound` is unreachable via the CLI (argparse `--style` choices
/// fire first) and is folded into the usage-error path by the caller.
pub enum HookInstallError {
    /// `NotAGitWorkTree` — no `.git` DIRECTORY (a `.git` file — a worktree
    /// or submodule pointer — fails too). Usage error, exit 2.
    NotAGitWorkTree(String),
    /// `HookFileExists` — refused; the existing hook is untouched (exit 1).
    FileExists(String),
    /// Filesystem write failure.
    Io(String),
}

/// `install_hook(target_dir, style)` — write the bundled `style` script to
/// `<dir>/.git/hooks/<style>` and make it executable (git requires the exec
/// bit; the oracle ORs `S_IXUSR|S_IXGRP|S_IXOTH` onto the fresh file's mode,
/// yielding 0755 under the usual umask).
pub fn install_hook(target_dir: &str, style: &str) -> Result<InstalledHook, HookInstallError> {
    let content = hook_bytes(style).expect("argparse-validated style");

    let git_dir = Path::new(target_dir).join(".git");
    if !git_dir.is_dir() {
        return Err(HookInstallError::NotAGitWorkTree(format!(
            "no .git directory in {target_dir}; run `decided hook install` from a git repository root"
        )));
    }

    let dest_display = py_join(target_dir, &[".git", "hooks", style]);
    let dest = Path::new(&dest_display);
    if dest.exists() {
        return Err(HookInstallError::FileExists(format!(
            "{dest_display} already exists; decided hook install never overwrites"
        )));
    }

    let hooks_dir = git_dir.join("hooks");
    std::fs::create_dir_all(&hooks_dir)
        .map_err(|e| HookInstallError::Io(format!("{e}: {}", hooks_dir.display())))?;
    std::fs::write(dest, content)
        .map_err(|e| HookInstallError::Io(format!("{e}: {dest_display}")))?;
    // dest.chmod(dest.stat().st_mode | S_IXUSR | S_IXGRP | S_IXOTH)
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(dest)
            .map_err(|e| HookInstallError::Io(format!("{e}: {dest_display}")))?
            .permissions();
        let new_mode = mode.mode() | 0o111;
        std::fs::set_permissions(dest, std::fs::Permissions::from_mode(new_mode))
            .map_err(|e| HookInstallError::Io(format!("{e}: {dest_display}")))?;
    }
    Ok(InstalledHook {
        style: style.to_string(),
        path: dest_display,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_order_and_default() {
        assert_eq!(available_hooks(), vec!["post-commit", "pre-commit"]);
        assert_eq!(DEFAULT_STYLE, "post-commit");
    }
}
