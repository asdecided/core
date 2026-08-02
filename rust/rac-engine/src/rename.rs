//! Safe artifact-id rename — `decided rename` (PORT-CONTRACT.d/16 §4).
//!
//! Port of `src/asdecided/services/rename.py` (`compute_rename`, `apply_rename`):
//! the deterministic, reversible corpus-wide edit set for renaming one
//! artifact identity. Resolution reuses the relationship validation alias
//! index; the raw reference TEXT is the source of truth — an edit replaces
//! exactly the `old_ref` token inside a relationship list line, preserving
//! everything around it, and only where the line NAMES that token (a line
//! naming a different alias of the same target is left untouched).

use std::collections::HashSet;
use std::fs;
use std::path::Path;
#[cfg(unix)]
use std::{fs::OpenOptions, io::Write};

use crate::pycompat::{py_casefold, py_is_space, py_splitlines, py_strip};
use crate::relationships::{
    corpus_items, resolution_index_from_rows, validation_row, CorpusItem, ValidationRow,
};
use crate::spec::RELATIONSHIP_SECTIONS;

// Stable reason codes for an invalid plan (part of the JSON contract).
pub const REASON_OLD_NOT_FOUND: &str = "old-ref-not-found";
pub const REASON_OLD_AMBIGUOUS: &str = "old-ref-ambiguous";
pub const REASON_NEW_COLLIDES: &str = "new-ref-collides";
pub const REASON_NEW_INVALID: &str = "new-ref-invalid";
pub const REASON_OLD_FILENAME_ONLY: &str = "old-ref-filename-only";
pub const REASON_SYMLINK_PATH: &str = "symlink-path";
pub const REASON_PATH_OUTSIDE_ROOT: &str = "path-outside-root";

// Where the rewritten identity token lived in the target file.
pub const IDENTITY_FRONTMATTER: &str = "frontmatter_id";
pub const IDENTITY_ID_SECTION: &str = "id_section";
pub const IDENTITY_ID_FIELD: &str = "id_field";

pub const KIND_REFERENCE: &str = "reference";
pub const KIND_IDENTITY: &str = "identity";

/// One line-level replacement (1-based `line`; exact line text without the
/// trailing newline).
pub struct RenameEdit {
    pub path: String,
    pub line: i64,
    pub old_line: String,
    pub new_line: String,
    pub kind: &'static str,
}

/// A deterministic, reversible corpus-wide rename edit set (ADR-007).
pub struct RenamePlan {
    pub directory: String,
    pub recursive: bool,
    pub old_ref: String,
    pub new_ref: String,
    pub ok: bool,
    pub target_path: Option<String>,
    pub identity_field: Option<&'static str>,
    pub reason: Option<&'static str>,
    pub edits: Vec<RenameEdit>,
}

impl RenamePlan {
    pub fn reference_edits(&self) -> usize {
        self.edits.iter().filter(|e| e.kind == KIND_REFERENCE).count()
    }

    pub fn identity_edits(&self) -> usize {
        self.edits.iter().filter(|e| e.kind == KIND_IDENTITY).count()
    }

    pub fn files_changed(&self) -> usize {
        self.edits
            .iter()
            .map(|e| e.path.as_str())
            .collect::<HashSet<_>>()
            .len()
    }
}

/// The outcome of applying a plan to disk.
pub struct RenameResult {
    pub directory: String,
    pub old_ref: String,
    pub new_ref: String,
    pub applied: bool,
    pub files_changed: usize,
    pub reference_edits: usize,
    pub identity_edits: usize,
    pub target_path: Option<String>,
}

#[derive(Debug)]
struct PathIssue {
    reason: &'static str,
    path: String,
    detail: String,
}

/// Check one path that a rename may mutate. The canonical root is resolved
/// once for the plan and again for application; the final path itself must
/// not be a symlink, and its resolved target must remain under that root.
///
/// The walker intentionally yields symlinked Markdown files for read-only
/// parity, but rename is a mutation surface: following one here could write
/// through to an unrelated file outside the requested corpus.
fn check_mutation_path(root: &Path, path: &str) -> Result<(), PathIssue> {
    let candidate = Path::new(path);
    let metadata = fs::symlink_metadata(candidate).map_err(|error| PathIssue {
        reason: REASON_PATH_OUTSIDE_ROOT,
        path: path.to_string(),
        detail: format!("cannot inspect path: {error}"),
    })?;
    if metadata.file_type().is_symlink() {
        return Err(PathIssue {
            reason: REASON_SYMLINK_PATH,
            path: path.to_string(),
            detail: "symlinked mutation paths are not permitted".to_string(),
        });
    }
    if !metadata.file_type().is_file() {
        return Err(PathIssue {
            reason: REASON_PATH_OUTSIDE_ROOT,
            path: path.to_string(),
            detail: "mutation paths must be regular files".to_string(),
        });
    }
    let canonical = fs::canonicalize(candidate).map_err(|error| PathIssue {
        reason: REASON_PATH_OUTSIDE_ROOT,
        path: path.to_string(),
        detail: format!("cannot resolve path: {error}"),
    })?;
    if !canonical.starts_with(root) {
        return Err(PathIssue {
            reason: REASON_PATH_OUTSIDE_ROOT,
            path: path.to_string(),
            detail: format!(
                "resolved path {} is outside corpus root {}",
                canonical.display(),
                root.display()
            ),
        });
    }
    Ok(())
}

fn path_issue_message(phase: &str, issue: &PathIssue) -> String {
    format!(
        "rename: refusing to {phase} {}: {}",
        issue.path, issue.detail
    )
}

/// Write an already-validated mutation path without following a last-second
/// final-component symlink on Unix. The preceding containment check still
/// protects the root boundary and is repeated immediately before this call.
fn write_mutation_file(path: &str, text: &str) -> Result<(), std::io::Error> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        let mut options = OpenOptions::new();
        options
            .write(true)
            .truncate(true)
            .custom_flags(libc::O_NOFOLLOW);
        let mut file = options.open(path)?;
        file.write_all(text.as_bytes())
    }
    #[cfg(not(unix))]
    {
        fs::write(path, text)
    }
}

fn refused(
    directory: &str,
    recursive: bool,
    old_ref: &str,
    new_ref: &str,
    target_path: Option<String>,
    reason: &'static str,
) -> RenamePlan {
    RenamePlan {
        directory: directory.to_string(),
        recursive,
        old_ref: old_ref.to_string(),
        new_ref: new_ref.to_string(),
        ok: false,
        target_path,
        identity_field: None,
        reason: Some(reason),
        edits: Vec::new(),
    }
}

/// `_NEW_REF_RE = ^[A-Za-z][\w.-]*$` — an ASCII letter, then Unicode word
/// chars / `.` / `-`. (`new_ref` is stripped first, so the Python `$`
/// trailing-newline allowance cannot fire.)
fn valid_new_ref(new_ref: &str) -> bool {
    let mut chars = new_ref.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() => {}
        _ => return false,
    }
    chars.all(|c| c.is_alphanumeric() || matches!(c, '_' | '.' | '-'))
}

/// `_replace_token(text, old_ref, new_ref)` — replace the LEADING token,
/// case-insensitively, whole-token (the next char must not be an
/// identifier char), preserving everything after it. `old_ref` arrives
/// pre-casefolded on the identity paths, exactly like the oracle.
fn replace_token(text: &str, old_ref: &str, new_ref: &str) -> Option<String> {
    let folded_old = py_casefold(old_ref);
    // `text[: len(old_ref)]` — Python slices by CHARS and simply yields a
    // shorter prefix when text is shorter; casefold-expansion can still
    // match it (the deliberate length quirk the oracle documents).
    let n = old_ref.chars().count();
    let byte_len: usize = text.chars().take(n).map(char::len_utf8).sum();
    if py_casefold(&text[..byte_len]) != folded_old {
        return None;
    }
    let rest = &text[byte_len..];
    if let Some(c) = rest.chars().next() {
        if c.is_alphanumeric() || matches!(c, '_' | '-' | '.') {
            return None;
        }
    }
    Some(format!("{new_ref}{rest}"))
}

/// `_LIST_MARKER_RE = ^(\s*(?:[-*+]|\d+\.)\s+)(.*)$` — returns the byte
/// length of group 1 (marker prefix incl. surrounding whitespace), or None.
fn list_marker_prefix_len(raw: &str) -> Option<usize> {
    let mut i = 0;
    for c in raw.chars() {
        if py_is_space(c) {
            i += c.len_utf8();
        } else {
            break;
        }
    }
    let rest = &raw[i..];
    let mut marker_len = 0;
    let mut chars = rest.chars();
    match chars.next() {
        Some(c @ ('-' | '*' | '+')) => marker_len += c.len_utf8(),
        Some(c) if crate::pycompat::is_re_digit(c) => {
            marker_len += c.len_utf8();
            loop {
                match rest[marker_len..].chars().next() {
                    Some(d) if crate::pycompat::is_re_digit(d) => marker_len += d.len_utf8(),
                    Some('.') => {
                        marker_len += 1;
                        break;
                    }
                    _ => return None,
                }
            }
        }
        _ => return None,
    }
    let tail = &rest[marker_len..];
    let ws: usize = tail
        .chars()
        .take_while(|c| py_is_space(*c))
        .map(char::len_utf8)
        .sum();
    if ws == 0 {
        return None;
    }
    Some(i + marker_len + ws)
}

/// Raw file text for one corpus path. The oracle's strict
/// `read_text(encoding="utf-8")` would traceback on invalid UTF-8; the
/// walk already decoded these files leniently, so a lossy decode here is
/// the same documented divergence class (stderr-only).
fn read_raw(path: &str) -> String {
    match std::fs::read(path) {
        Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
        Err(_) => String::new(),
    }
}

/// `_relationship_reference_lines(raw_lines, sections)` — 1-based
/// `(line_no, raw_line)` for every non-empty line inside a relevant
/// relationship section, tracking the current `##` heading.
fn relationship_reference_lines<'a>(
    raw_lines: &[&'a str],
    sections: &HashSet<String>,
) -> Vec<(usize, &'a str)> {
    let mut result = Vec::new();
    let mut current: Option<String> = None;
    for (i, raw) in raw_lines.iter().enumerate() {
        let stripped = py_strip(raw);
        if let Some(rest) = stripped.strip_prefix("## ") {
            current = Some(py_casefold(py_strip(rest)));
            continue;
        }
        if stripped.starts_with('#') {
            current = None; // any other heading ends the section
            continue;
        }
        if let Some(cur) = &current {
            if sections.contains(cur) && !stripped.is_empty() {
                result.push((i + 1, *raw));
            }
        }
    }
    result
}

/// `_reference_edits(items, target_path, old_ref, new_ref)` — every inbound
/// relationship line whose leading reference token equals `old_ref`.
fn reference_edits(
    items: &[CorpusItem],
    root: &Path,
    old_ref: &str,
    new_ref: &str,
) -> Result<Vec<RenameEdit>, PathIssue> {
    let mut edits = Vec::new();
    for item in items {
        let Some(spec) = item.spec else { continue };
        let present: HashSet<String> = spec
            .optional
            .iter()
            .filter(|section| {
                RELATIONSHIP_SECTIONS.iter().any(|(name, _)| name == section)
                    && item
                        .artifact
                        .section(section)
                        .map(|body| !body.is_empty())
                        .unwrap_or(false)
            })
            .cloned()
            .collect();
        if present.is_empty() {
            continue;
        }
        check_mutation_path(root, &item.path)?;
        let raw = read_raw(&item.path);
        let raw_lines = py_splitlines(&raw);
        for (line_no, raw_line) in relationship_reference_lines(&raw_lines, &present) {
            let prefix_len = list_marker_prefix_len(raw_line).unwrap_or(0);
            let ref_text = &raw_line[prefix_len..];
            let Some(rewritten) = replace_token(py_strip(ref_text), old_ref, new_ref) else {
                continue;
            };
            // Preserve the marker and surrounding whitespace by rebuilding
            // only the reference portion (first occurrence in the tail).
            let new_line = format!(
                "{}{}",
                &raw_line[..prefix_len],
                raw_line[prefix_len..].replacen(py_strip(ref_text), &rewritten, 1)
            );
            if new_line != raw_line {
                edits.push(RenameEdit {
                    path: item.path.clone(),
                    line: line_no as i64,
                    old_line: raw_line.to_string(),
                    new_line,
                    kind: KIND_REFERENCE,
                });
            }
        }
    }
    Ok(edits)
}

/// One matched frontmatter `id:` line: `(g1 prefix, g2 quote, g3 value,
/// g5 suffix)` per `_FRONTMATTER_ID_RE` — value may be quoted, a trailing
/// `#` comment is preserved.
fn frontmatter_id_line(line: &str) -> Option<(String, String, String, String)> {
    let mut i = 0;
    for c in line.chars() {
        if py_is_space(c) {
            i += c.len_utf8();
        } else {
            break;
        }
    }
    let after_ws = &line[i..];
    if !after_ws.starts_with("id") {
        return None;
    }
    i += 2;
    for c in line[i..].chars() {
        if py_is_space(c) {
            i += c.len_utf8();
        } else {
            break;
        }
    }
    if !line[i..].starts_with(':') {
        return None;
    }
    i += 1;
    for c in line[i..].chars() {
        if py_is_space(c) {
            i += c.len_utf8();
        } else {
            break;
        }
    }
    let g1 = line[..i].to_string();
    let rest = &line[i..];
    let quote = match rest.chars().next() {
        Some(q @ ('\'' | '"')) => Some(q),
        _ => None,
    };
    if let Some(q) = quote {
        // Quoted: value is [^'"#]+ up to the SAME quote, then ws + optional
        // comment. Any quote or '#' inside the value fails the whole match.
        let body = &rest[q.len_utf8()..];
        let mut vlen = 0;
        let mut closed = false;
        for c in body.chars() {
            if c == q {
                closed = true;
                break;
            }
            if matches!(c, '\'' | '"' | '#') {
                return None;
            }
            vlen += c.len_utf8();
        }
        if !closed || vlen == 0 {
            return None;
        }
        let after = &body[vlen + q.len_utf8()..];
        if !ws_then_optional_comment(after) {
            return None;
        }
        Some((g1, q.to_string(), body[..vlen].to_string(), after.to_string()))
    } else {
        // Unquoted: a quote anywhere before the comment fails; the lazy
        // group pushes trailing whitespace into the suffix.
        let mut vlen = 0;
        let mut has_hash = false;
        for c in rest.chars() {
            if c == '#' {
                has_hash = true;
                break;
            }
            if matches!(c, '\'' | '"') {
                return None;
            }
            vlen += c.len_utf8();
        }
        let run = &rest[..vlen];
        let value = run.trim_end_matches(py_is_space);
        if value.is_empty() {
            // A pure-whitespace value strips to "", which can never equal a
            // non-empty old_ref — no edit either way.
            return None;
        }
        let suffix = format!(
            "{}{}",
            &run[value.len()..],
            if has_hash { &rest[vlen..] } else { "" }
        );
        Some((g1, String::new(), value.to_string(), suffix))
    }
}

fn ws_then_optional_comment(s: &str) -> bool {
    let rest = s.trim_start_matches(py_is_space);
    rest.is_empty() || rest.starts_with('#')
}

/// Partial edit: (1-based line, old text, new text).
type LineEdit = (usize, String, String);

/// `_frontmatter_id_edit(raw_lines, old_ref, new_ref)` — rewrite the value
/// of the `id:` line inside the LEADING `---` block.
fn frontmatter_id_edit(raw_lines: &[&str], old_ref: &str, new_ref: &str) -> Option<LineEdit> {
    if raw_lines.first().map(|l| py_strip(l)) != Some("---") {
        return None;
    }
    for (i, raw) in raw_lines.iter().enumerate().skip(1) {
        if py_strip(raw) == "---" {
            break;
        }
        if let Some((g1, g2, g3, g5)) = frontmatter_id_line(raw) {
            if py_casefold(py_strip(&g3)) == py_casefold(old_ref) {
                let new_line = format!("{g1}{g2}{new_ref}{g2}{g5}");
                if new_line != *raw {
                    return Some((i + 1, (*raw).to_string(), new_line));
                }
            }
        }
    }
    None
}

/// `^\s*##\s+<name>\s*$` (IGNORECASE) — the `## ID` / `## <id_field>`
/// heading matcher.
fn heading_matches(raw: &str, name: &str) -> bool {
    let after_ws = raw.trim_start_matches(py_is_space);
    let Some(rest) = after_ws.strip_prefix("##") else {
        return false;
    };
    let after_hash_ws = rest.trim_start_matches(py_is_space);
    if after_hash_ws.len() == rest.len() {
        return false; // \s+ requires at least one space after ##
    }
    let n = name.chars().count();
    let byte_len: usize = after_hash_ws.chars().take(n).map(char::len_utf8).sum();
    if after_hash_ws.chars().count() < n
        || py_casefold(&after_hash_ws[..byte_len]) != py_casefold(name)
    {
        return false;
    }
    after_hash_ws[byte_len..].chars().all(py_is_space)
}

/// `_section_first_value_edit` — rewrite the first value line under a
/// matching heading; only the FIRST value line of each matching section is
/// the identity (scanning continues after a non-rewriting first value).
fn section_first_value_edit(
    raw_lines: &[&str],
    section_name: &str,
    folded_old: &str,
    new_ref: &str,
) -> Option<LineEdit> {
    let mut in_section = false;
    for (i, raw) in raw_lines.iter().enumerate() {
        let stripped = py_strip(raw);
        if stripped.starts_with('#') {
            in_section = heading_matches(raw, section_name);
            continue;
        }
        if !in_section || stripped.is_empty() {
            continue;
        }
        let prefix_len = list_marker_prefix_len(raw).unwrap_or(0);
        let value = &raw[prefix_len..];
        let rewritten = replace_token(py_strip(value), folded_old, new_ref);
        in_section = false; // only the first value line is the identity
        let Some(rewritten) = rewritten else { continue };
        let new_line = format!(
            "{}{}",
            &raw[..prefix_len],
            raw[prefix_len..].replacen(py_strip(value), &rewritten, 1)
        );
        if new_line != *raw {
            return Some((i + 1, (*raw).to_string(), new_line));
        }
    }
    None
}

/// `_identity_edit(target_path, product, spec, old_ref, new_ref)` —
/// `(edit, identity_field)` on success, or the filename-only refusal.
fn identity_edit(
    item: &CorpusItem,
    old_ref: &str,
    new_ref: &str,
) -> Result<(LineEdit, &'static str), &'static str> {
    let raw = read_raw(&item.path);
    let raw_lines = py_splitlines(&raw);
    let folded_old = py_casefold(old_ref);

    // 1. Canonical frontmatter `id` — only when old_ref IS it.
    if let Some(meta) = &item.artifact.metadata {
        if let Some(id) = meta.id.as_deref().filter(|s| !s.is_empty()) {
            if py_casefold(id) == folded_old {
                if let Some(edit) = frontmatter_id_edit(&raw_lines, old_ref, new_ref) {
                    return Ok((edit, IDENTITY_FRONTMATTER));
                }
            }
        }
    }

    // 2. `## ID` section value.
    if let Some(edit) = section_first_value_edit(&raw_lines, "id", &folded_old, new_ref) {
        return Ok((edit, IDENTITY_ID_SECTION));
    }

    // 3. The type's `spec.id_field` section (no spec sets one — ported for
    //    fidelity, dead today).
    if let Some(spec) = item.spec {
        if let Some(field) = spec.id_field.as_deref().filter(|f| !f.is_empty()) {
            if let Some(edit) = section_first_value_edit(&raw_lines, field, &folded_old, new_ref) {
                return Ok((edit, IDENTITY_ID_FIELD));
            }
        }
    }

    // 4. Filename-derived alias only — nothing editable in-file.
    Err(REASON_OLD_FILENAME_ONLY)
}

/// `compute_rename(directory, old_ref, new_ref, recursive)`.
pub fn compute_rename(
    directory: &str,
    old_ref: &str,
    new_ref: &str,
    recursive: bool,
) -> RenamePlan {
    let new_ref = py_strip(new_ref).to_string();
    if !valid_new_ref(&new_ref) {
        return refused(directory, recursive, old_ref, &new_ref, None, REASON_NEW_INVALID);
    }

    let root = match fs::canonicalize(directory) {
        Ok(root) => root,
        Err(_) => {
            return refused(
                directory,
                recursive,
                old_ref,
                &new_ref,
                Some(directory.to_string()),
                REASON_PATH_OUTSIDE_ROOT,
            )
        }
    };

    let items = corpus_items(directory, recursive);
    let rows: Vec<ValidationRow> = items
        .iter()
        .map(|item| validation_row(&item.path, &item.artifact, item.spec))
        .collect();
    let index = resolution_index_from_rows(&rows);
    let mut targets: Vec<&str> = index
        .get(&py_casefold(old_ref))
        .iter()
        .map(|(path, _)| path.as_str())
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    targets.sort_unstable();
    let target_path = match targets.as_slice() {
        [] => {
            return refused(directory, recursive, old_ref, &new_ref, None, REASON_OLD_NOT_FOUND)
        }
        [one] => (*one).to_string(),
        _ => {
            return refused(directory, recursive, old_ref, &new_ref, None, REASON_OLD_AMBIGUOUS)
        }
    };

    // A no-op rename (new == old case-insensitively) skips the collision
    // check; otherwise a `new_ref` naming ANOTHER artifact refuses.
    if py_casefold(&new_ref) != py_casefold(old_ref) {
        let folded_new = py_casefold(&new_ref);
        let collides = rows.iter().any(|row| {
            row.path != target_path
                && row.identifiers.iter().any(|i| py_casefold(i) == folded_new)
        });
        if collides {
            return refused(
                directory,
                recursive,
                old_ref,
                &new_ref,
                Some(target_path),
                REASON_NEW_COLLIDES,
            );
        }
    }

    let target_item = items
        .iter()
        .find(|item| item.path == target_path)
        .expect("resolved target is in the walked corpus");
    if let Err(issue) = check_mutation_path(&root, &target_path) {
        return refused(
            directory,
            recursive,
            old_ref,
            &new_ref,
            Some(issue.path),
            issue.reason,
        );
    }
    let (identity, identity_field) = match identity_edit(target_item, old_ref, &new_ref) {
        Ok(pair) => pair,
        Err(reason) => {
            return refused(directory, recursive, old_ref, &new_ref, Some(target_path), reason)
        }
    };

    let mut edits = match reference_edits(&items, &root, old_ref, &new_ref) {
        Ok(edits) => edits,
        Err(issue) => {
            return refused(
                directory,
                recursive,
                old_ref,
                &new_ref,
                Some(issue.path),
                issue.reason,
            )
        }
    };
    let (line, old_line, new_line) = identity;
    edits.push(RenameEdit {
        path: target_path.clone(),
        line: line as i64,
        old_line,
        new_line,
        kind: KIND_IDENTITY,
    });
    edits.sort_by(|a, b| (a.path.as_str(), a.line).cmp(&(b.path.as_str(), b.line)));

    RenamePlan {
        directory: directory.to_string(),
        recursive,
        old_ref: old_ref.to_string(),
        new_ref,
        ok: true,
        target_path: Some(target_path),
        identity_field: Some(identity_field),
        reason: None,
        edits,
    }
}

/// `apply_rename(plan)` — exact line replacements, original final-newline
/// shape preserved. A stale plan (the file changed since it was computed)
/// is the oracle's uncaught `ValueError` traceback (exit 1); surfaced here
/// as `Err(message)` for the command to fail with the same code.
pub fn apply_rename(plan: &RenamePlan) -> Result<RenameResult, String> {
    if !plan.ok {
        return Ok(RenameResult {
            directory: plan.directory.clone(),
            old_ref: plan.old_ref.clone(),
            new_ref: plan.new_ref.clone(),
            applied: false,
            files_changed: 0,
            reference_edits: 0,
            identity_edits: 0,
            target_path: plan.target_path.clone(),
        });
    }

    let root = fs::canonicalize(&plan.directory)
        .map_err(|e| format!("rename: cannot resolve corpus root {}: {e}", plan.directory))?;

    // Group by path, first-seen order (Python dict setdefault).
    let mut order: Vec<&str> = Vec::new();
    for edit in &plan.edits {
        if !order.contains(&edit.path.as_str()) {
            order.push(&edit.path);
        }
    }
    for path in order {
        check_mutation_path(&root, path).map_err(|issue| path_issue_message("read", &issue))?;
        let original = std::fs::read_to_string(path)
            .map_err(|e| format!("rename: cannot read {path}: {e}"))?;
        let had_final_newline = original.ends_with('\n');
        let mut lines: Vec<String> = py_splitlines(&original)
            .into_iter()
            .map(str::to_string)
            .collect();
        for edit in plan.edits.iter().filter(|e| e.path == path) {
            let idx = edit.line - 1;
            let in_range = idx >= 0 && (idx as usize) < lines.len();
            if !in_range || lines[idx as usize] != edit.old_line {
                return Err(format!(
                    "rename: stale plan for {path} line {}: file changed since the plan was computed",
                    edit.line
                ));
            }
            lines[idx as usize] = edit.new_line.clone();
        }
        let mut text = lines.join("\n");
        if had_final_newline {
            text.push('\n');
        }
        check_mutation_path(&root, path).map_err(|issue| path_issue_message("replace", &issue))?;
        write_mutation_file(path, &text)
            .map_err(|e| format!("rename: cannot write {path}: {e}"))?;
    }

    Ok(RenameResult {
        directory: plan.directory.clone(),
        old_ref: plan.old_ref.clone(),
        new_ref: plan.new_ref.clone(),
        applied: true,
        files_changed: plan.files_changed(),
        reference_edits: plan.reference_edits(),
        identity_edits: plan.identity_edits(),
        target_path: plan.target_path.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_ref_grammar() {
        assert!(valid_new_ref("ADR-099"));
        assert!(valid_new_ref("RAC-ZZZZZZZZZZZZ"));
        assert!(valid_new_ref("a.b-c_d1"));
        assert!(!valid_new_ref(""));
        assert!(!valid_new_ref("1AD"));
        assert!(!valid_new_ref("bad id!"));
        assert!(!valid_new_ref("-x"));
    }

    #[test]
    fn token_replacement_is_whole_token_and_case_insensitive() {
        assert_eq!(
            replace_token("ADR-001 (blocked)", "adr-001", "ADR-099").as_deref(),
            Some("ADR-099 (blocked)")
        );
        assert_eq!(replace_token("ADR-10", "ADR-1", "X"), None);
        assert_eq!(replace_token("ADR-1.5", "ADR-1", "X"), None);
        assert_eq!(replace_token("zzz", "ADR-1", "X"), None);
    }

    #[test]
    fn frontmatter_id_line_shapes() {
        assert_eq!(
            frontmatter_id_line("id: RAC-A"),
            Some(("id: ".into(), "".into(), "RAC-A".into(), "".into()))
        );
        assert_eq!(
            frontmatter_id_line("  id:  'RAC-A'  # note"),
            Some(("  id:  ".into(), "'".into(), "RAC-A".into(), "  # note".into()))
        );
        assert_eq!(frontmatter_id_line("id: 'RAC"), None);
        // "ident" begins with the literal `id`, but the regex then demands
        // `\s*:` and finds 'e' — no match. Same for a missing colon.
        assert_eq!(frontmatter_id_line("ident: RAC-A"), None);
        assert_eq!(frontmatter_id_line("id RAC-A"), None);
    }
}
