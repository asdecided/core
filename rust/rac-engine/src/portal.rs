//! Portal HTML assembly (`decided.output.portal`) — inject the export payload
//! into the vendored shell, per PORT-CONTRACT.d/17 §2.
//!
//! The legacy shell is retained byte-for-byte for repositories without a
//! federation manifest. Federated exports use the source-aware shell. Both
//! assets are embedded at compile time and share the same payload seam.

use crate::export::CorpusExport;

/// The prerequisite Portal shell, embedded verbatim for no-manifest output.
pub const SHELL: &str = include_str!("../assets/portal/asdecided-portal-legacy-shell.html");

/// The source-aware Portal shell used only for a composed federated export.
pub const FEDERATED_SHELL: &str = include_str!("../assets/portal/asdecided-portal-shell.html");

/// The exact empty data seam the shell-only viewer build emits (no
/// whitespace inside the element); the populated form replaces it verbatim.
const SEAM: &str = r#"<script type="application/json" id="lore-export"></script>"#;

/// `_escape_for_script(payload)` — make serialized JSON safe inside a
/// `<script>` element with two valid JSON escapes, applied in the oracle's
/// order: `</` → `<\/` first, then `<!--` → `<\u{0021}--` (both literal
/// `str.replace` passes over the whole serialized document).
fn escape_for_script(payload: &str) -> String {
    payload.replace("</", "<\\/").replace("<!--", "<\\u0021--")
}

/// `render_export_html(export)` — the vendored shell with the export JSON
/// injected into its single data seam.
///
/// `PortalShellMissing` is unreachable with a compile-time embed; the
/// `PortalSeamMissing` guard is retained for contract fidelity (its message
/// bytes feed `decided: <msg>`, exit 2). Both are operational errors in the
/// oracle, not caller errors.
fn render_export_html_with_shell(export: &CorpusExport, shell: &str) -> Result<String, String> {
    if shell.matches(SEAM).count() != 1 {
        return Err(format!(
            "packaged portal shell has no usable data seam \
             ({SEAM}); re-vendor it: cd decided-localview && npm run vendor:shell"
        ));
    }
    let payload = escape_for_script(&crate::output::render_export_json(export));
    let populated =
        format!(r#"<script type="application/json" id="lore-export">{payload}</script>"#);
    // `str.replace` on a count-1 needle: a single substitution.
    Ok(shell.replacen(SEAM, &populated, 1))
}

pub fn render_export_html(export: &CorpusExport) -> Result<String, String> {
    render_export_html_with_shell(export, SHELL)
}

pub fn render_federated_export_html(export: &CorpusExport) -> Result<String, String> {
    render_export_html_with_shell(export, FEDERATED_SHELL)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_carries_exactly_one_empty_seam() {
        assert_eq!(SHELL.matches(SEAM).count(), 1);
        assert_eq!(FEDERATED_SHELL.matches(SEAM).count(), 1);
        assert_ne!(SHELL, FEDERATED_SHELL);
    }

    /// The two escapes in the oracle's order; the comment-open rewrite must
    /// not be disturbed by the `</` pass.
    #[test]
    fn escape_order_and_exactness() {
        assert_eq!(
            escape_for_script(r#"a</script>b<!--c"#),
            r#"a<\/script>b<\u0021--c"#
        );
        assert_eq!(escape_for_script("plain"), "plain");
    }
}
