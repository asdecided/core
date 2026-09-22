//! Deterministic classification (`decided.core.classification`), per
//! PORT-CONTRACT.d/04 §2.
//!
//! - synonym-aware section mapping is per-spec (`_mapped`), a *set*;
//! - scoring floats replicate the exact Python arithmetic (`len_req +
//!   0.5 * len_rec`, then divide);
//! - the sort is `sort(key=(fit, len(matched_required)), reverse=True)` —
//!   stable, ties preserve ARTIFACT_SPECS order (reverse flips key order but
//!   NOT equal-key runs);
//! - `confidence = round(fit, 2)` — banker's rounding on the true double
//!   (`pycompat::py_round`).

use crate::parse::Artifact;
use crate::pycompat::py_round;
use crate::spec::{specs, ArtifactSpec};

pub const CONFIDENCE_THRESHOLD: f64 = 0.5;

/// How well a document fits one artifact type (`TypeScore`).
#[derive(Debug, Clone)]
pub struct TypeScore {
    pub name: String,
    pub matched_required: Vec<String>,
    pub matched_recommended: Vec<String>,
    pub missing: Vec<String>,
    pub points: f64,
    pub ceiling: f64,
    pub fit: f64,
}

/// The chosen artifact type for a document (or Unknown).
#[derive(Debug, Clone)]
pub struct Classification {
    /// Artifact name, or `"unknown"`.
    pub artifact_type: String,
    /// `round(fit, 2)`.
    pub confidence: f64,
    pub present_sections: Vec<String>,
    pub missing_sections: Vec<String>,
}

/// `_mapped(product, spec)`: the document's normalized headings with this
/// spec's synonyms applied — set semantics (duplicates collapse; membership
/// is all that matters downstream).
fn mapped<'a>(artifact: &'a Artifact, spec: &'a ArtifactSpec) -> Vec<&'a str> {
    let mut out: Vec<&str> = Vec::new();
    for (heading, _) in &artifact.product.sections {
        let m = spec.synonym(heading).unwrap_or(heading.as_str());
        if !out.contains(&m) {
            out.push(m);
        }
    }
    out
}

/// `missing_sections(product, spec)` -> `(missing_required, missing_recommended)`
/// in schema declaration order, synonym-aware.
pub fn missing_sections(artifact: &Artifact, spec: &ArtifactSpec) -> (Vec<String>, Vec<String>) {
    let m = mapped(artifact, spec);
    let missing_required = spec
        .required
        .iter()
        .filter(|s| !m.contains(&s.as_str()))
        .cloned()
        .collect();
    let missing_recommended = spec
        .recommended
        .iter()
        .filter(|s| !m.contains(&s.as_str()))
        .cloned()
        .collect();
    (missing_required, missing_recommended)
}

/// `score_artifacts(product)`: scores best-fit-first with the exact Python
/// sort semantics.
pub fn score_artifacts(artifact: &Artifact) -> Vec<TypeScore> {
    let mut scores: Vec<TypeScore> = Vec::new();
    for spec in specs() {
        let m = mapped(artifact, spec);
        let matched_required: Vec<String> = spec
            .required
            .iter()
            .filter(|s| m.contains(&s.as_str()))
            .cloned()
            .collect();
        let matched_recommended: Vec<String> = spec
            .recommended
            .iter()
            .filter(|s| m.contains(&s.as_str()))
            .cloned()
            .collect();
        // `expected` (Python property) = `required + recommended`, in that order.
        let missing: Vec<String> = spec
            .required
            .iter()
            .chain(spec.recommended.iter())
            .filter(|s| !m.contains(&s.as_str()))
            .cloned()
            .collect();
        let points = matched_required.len() as f64 + 0.5 * matched_recommended.len() as f64;
        let ceiling = spec.required.len() as f64 + 0.5 * spec.recommended.len() as f64;
        let fit = if ceiling != 0.0 {
            points / ceiling
        } else {
            0.0
        };
        scores.push(TypeScore {
            name: spec.name.clone(),
            matched_required,
            matched_recommended,
            missing,
            points,
            ceiling,
            fit,
        });
    }
    // Python: scores.sort(key=lambda t: (t.fit, len(t.matched_required)),
    // reverse=True) — descending by key, equal keys keep ORIGINAL order.
    // Implemented as a stable sort on the descending comparison only (equal
    // keys compare Equal, so stability preserves registry order).
    scores.sort_by(|a, b| {
        b.fit
            .partial_cmp(&a.fit)
            .unwrap()
            .then(b.matched_required.len().cmp(&a.matched_required.len()))
    });
    scores
}

/// `classify(product)`.
pub fn classify(artifact: &Artifact) -> Classification {
    let scores = score_artifacts(artifact);
    let qualifies =
        |score: &TypeScore| score.fit >= CONFIDENCE_THRESHOLD && !score.matched_required.is_empty();
    // Built-ins always win (ADR-083): a bundle type ranked first takes a
    // document only when no built-in qualifies for it, so a bundle can never
    // take classification away from a built-in artifact. With no bundle the
    // first score is always a built-in and nothing here changes.
    let best = match scores.first() {
        Some(first) if !crate::spec::is_builtin(&first.name) => scores
            .iter()
            .find(|score| crate::spec::is_builtin(&score.name) && qualifies(score))
            .unwrap_or(first),
        _ => &scores[0], // 5 specs -> never empty
    };
    if best.fit < CONFIDENCE_THRESHOLD || best.matched_required.is_empty() {
        return Classification {
            artifact_type: "unknown".to_string(),
            confidence: py_round(best.fit, 2),
            present_sections: artifact
                .product
                .sections
                .iter()
                .map(|(h, _)| h.clone())
                .collect(),
            missing_sections: Vec::new(),
        };
    }
    let mut present = best.matched_required.clone();
    present.extend(best.matched_recommended.iter().cloned());
    Classification {
        artifact_type: best.name.clone(),
        confidence: py_round(best.fit, 2),
        present_sections: present,
        missing_sections: best.missing.clone(),
    }
}
