//! Bundled agent skills — `decided skill` (PORT-CONTRACT.d/15).
//!
//! Port of `src/rac/core/skills.py` (registry + resource loading) and
//! `src/asdecided/services/skill.py` (`install_skills`). The packaged `SKILL.md`
//! resources are embedded verbatim from `rust/decided-engine/assets/skills/`,
//! vendored byte-identical copies of the Python package files — a unit test
//! below pins that identity, because the installed file must be
//! byte-identical to what the oracle installs (skill brief, landmine 1).
//!
//! `SkillResourceMissing` (a broken Python installation) has no Rust
//! equivalent: embedded resources cannot be absent from a linked binary.

use std::path::Path;

use crate::walk::py_join;

/// One bundled skill: name and one-line description, registry order.
pub struct SkillSpec {
    pub name: &'static str,
    pub description: &'static str,
}

/// Bundled skills, in registry order (`BUNDLED_SKILLS`). `install` with no
/// name installs all of them; `list` enumerates them.
pub const BUNDLED_SKILLS: [SkillSpec; 4] = [
    SkillSpec {
        name: "decided-artifacts",
        description: "Author and maintain AsDecided Markdown artifacts with the decided CLI.",
    },
    SkillSpec {
        name: "decided-review",
        description: "Review an AsDecided corpus and work findings worst-first.",
    },
    SkillSpec {
        name: "decided-import",
        description: "Reformat one document into one valid AsDecided artifact, with human review.",
    },
    SkillSpec {
        name: "decided-capture",
        description: "Capture a new decision or requirement into a valid AsDecided artifact.",
    },
];

/// The embedded `SKILL.md` bytes, index-aligned with [`BUNDLED_SKILLS`].
pub(crate) const SKILL_BYTES: [&[u8]; 4] = [
    include_bytes!("../assets/skills/decided-artifacts/SKILL.md"),
    include_bytes!("../assets/skills/decided-review/SKILL.md"),
    include_bytes!("../assets/skills/decided-import/SKILL.md"),
    include_bytes!("../assets/skills/decided-capture/SKILL.md"),
];

/// `available_skills()` — bundled skill names, registry order.
pub fn available_skills() -> Vec<&'static str> {
    BUNDLED_SKILLS.iter().map(|s| s.name).collect()
}

fn skill_bytes(name: &str) -> Option<&'static [u8]> {
    BUNDLED_SKILLS
        .iter()
        .position(|s| s.name == name)
        .map(|i| SKILL_BYTES[i])
}

/// One installed skill (`InstalledSkill`; `bytes_written` is in the oracle's
/// model but deliberately absent from its JSON, so it is not carried here).
pub struct InstalledSkill {
    pub skill: String,
    pub path: String,
}

/// Result of a `decided skill install` run.
pub struct SkillInstallation {
    pub skills: Vec<InstalledSkill>,
}

/// The failure contract of `install_skills`, message-shaped like the oracle.
pub enum SkillInstallError {
    /// `SkillNotFound` — unregistered name (CLI usage error, exit 2).
    NotFound(String),
    /// `SkillFileExists` — refused before anything is written (exit 1).
    FileExists(String),
    /// Filesystem write failure (the oracle would raise `OSError`; carried
    /// so the CLI can fail loudly instead of pretending success).
    Io(String),
}

/// Resolve every destination an installation would write, preserving the
/// registry-order and unknown-name contract. Callers can preflight these
/// concrete paths before the all-or-nothing write loop begins.
pub fn install_targets(
    target_dir: &str,
    skill_name: Option<&str>,
) -> Result<Vec<String>, SkillInstallError> {
    if let Some(name) = skill_name {
        if skill_bytes(name).is_none() {
            return Err(SkillInstallError::NotFound(format!(
                "unknown skill: {name} (available: {})",
                available_skills().join(", ")
            )));
        }
    }
    let names: Vec<&str> = match skill_name {
        Some(name) => vec![name],
        None => available_skills(),
    };
    Ok(names
        .iter()
        .map(|name| py_join(target_dir, &[".claude", "skills", name, "SKILL.md"]))
        .collect())
}

/// `install_skills(target_dir, skill_name)` — write bundled skills into
/// `<dir>/.claude/skills/<name>/SKILL.md`.
///
/// With no name every bundled skill is installed all-or-nothing: every
/// target path is checked BEFORE any write, and one collision refuses the
/// whole installation with nothing written (existing paths listed in
/// registry order). Emitted paths are `str(Path(dir) / ...)` — the caller's
/// `--dir` normalized by pathlib, never abspath'd (landmine 6).
pub fn install_skills(
    target_dir: &str,
    skill_name: Option<&str>,
) -> Result<SkillInstallation, SkillInstallError> {
    let names: Vec<&str> = match skill_name {
        Some(name) => vec![name],
        None => available_skills(),
    };

    // Check every destination first, then write — a refusal never leaves a
    // partial installation behind.
    let destinations = install_targets(target_dir, skill_name)?;
    let existing: Vec<&str> = destinations
        .iter()
        .filter(|dest| Path::new(dest.as_str()).exists())
        .map(String::as_str)
        .collect();
    if !existing.is_empty() {
        let message = if existing.len() == 1 {
            format!("{} already exists; decided skill install never overwrites", existing[0])
        } else {
            let listing: Vec<String> = existing.iter().map(|p| format!("  - {p}")).collect();
            format!(
                "{} skill files already exist; decided skill install never overwrites:\n{}",
                existing.len(),
                listing.join("\n")
            )
        };
        return Err(SkillInstallError::FileExists(message));
    }

    let mut installed: Vec<InstalledSkill> = Vec::new();
    for (name, dest) in names.iter().zip(&destinations) {
        let content = skill_bytes(name).expect("registered skill");
        let path = Path::new(dest.as_str());
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| SkillInstallError::Io(format!("{e}: {}", parent.display())))?;
        }
        std::fs::write(path, content)
            .map_err(|e| SkillInstallError::Io(format!("{e}: {dest}")))?;
        installed.push(InstalledSkill {
            skill: (*name).to_string(),
            path: dest.clone(),
        });
    }
    Ok(SkillInstallation { skills: installed })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_order_and_names() {
        assert_eq!(
            available_skills(),
            vec!["decided-artifacts", "decided-review", "decided-import", "decided-capture"]
        );
    }
}
