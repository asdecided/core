//! Central source-aware corpus composition (ADR-136 through ADR-138).
//!
//! This module is intentionally dormant at the command boundary. The parent
//! verifier supplies already-validated items and declaration values later;
//! composition owns the single catalog/effective overlay every reader will
//! consume. It performs no filesystem or network work.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt;

use crate::corpus::{ArtifactKey, ArtifactPath, Layer};
use crate::pycompat::py_casefold;
use crate::relationships::{
    resolution_index_from_rows, resolve_relationships, validation_from_rows_with_index,
    validation_row_from_item, CorpusItem, Relationship, RelationshipValidation,
    ResolutionCandidate, ResolutionIndex, ValidationRow,
};
use crate::resolve::is_live_decision;

pub const FINDING_CANONICAL_COLLISION: &str = "cross-corpus-canonical-id-collision";
pub const FINDING_INVALID_OVERRIDE: &str = "cross-corpus-invalid-override";

/// A canonical identifier used by a local override operand.
///
/// It deliberately cannot contain the qualified-reference delimiter. Whether
/// the value is truly canonical is then established by a direct lookup in the
/// appropriate layer; artifact aliases never participate in that lookup.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CanonicalId(String);

impl CanonicalId {
    pub fn new(value: impl Into<String>) -> Result<Self, OverrideSyntaxError> {
        let value = value.into();
        if value.is_empty() || value.trim() != value {
            return Err(OverrideSyntaxError::InvalidCanonicalId);
        }
        if value.contains("::") {
            return Err(OverrideSyntaxError::QualifiedLocalId);
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for CanonicalId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// A child-local parent alias plus a canonical parent identifier.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct QualifiedCanonicalId {
    alias: String,
    canonical_id: CanonicalId,
}

impl QualifiedCanonicalId {
    pub fn new(
        alias: impl Into<String>,
        canonical_id: CanonicalId,
    ) -> Result<Self, OverrideSyntaxError> {
        let alias = alias.into();
        if !valid_source_alias(&alias) {
            return Err(OverrideSyntaxError::InvalidSourceAlias);
        }
        Ok(Self {
            alias,
            canonical_id,
        })
    }

    pub fn parse(value: &str) -> Result<Self, OverrideSyntaxError> {
        let Some((alias, canonical_id)) = value.split_once("::") else {
            return Err(OverrideSyntaxError::ParentMustBeQualified);
        };
        if canonical_id.contains("::") {
            return Err(OverrideSyntaxError::InvalidQualifiedId);
        }
        Self::new(alias, CanonicalId::new(canonical_id)?)
    }

    pub fn alias(&self) -> &str {
        &self.alias
    }

    pub fn canonical_id(&self) -> &CanonicalId {
        &self.canonical_id
    }
}

impl fmt::Display for QualifiedCanonicalId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}::{}", self.alias, self.canonical_id)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverrideSyntaxError {
    InvalidCanonicalId,
    QualifiedLocalId,
    ParentMustBeQualified,
    InvalidQualifiedId,
    InvalidSourceAlias,
}

impl fmt::Display for OverrideSyntaxError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidCanonicalId => "canonical id must be non-empty and unpadded",
            Self::QualifiedLocalId => "local canonical id must not be qualified",
            Self::ParentMustBeQualified => "parent canonical id must be qualified",
            Self::InvalidQualifiedId => "qualified id must contain exactly one `::` delimiter",
            Self::InvalidSourceAlias => "source alias must be lowercase and path-free",
        })
    }
}

impl std::error::Error for OverrideSyntaxError {}

/// The source identity and child-local alias of the one verified parent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParentIdentity {
    pub source: String,
    pub alias: String,
}

impl ParentIdentity {
    pub fn new(
        source: impl Into<String>,
        alias: impl Into<String>,
    ) -> Result<Self, OverrideSyntaxError> {
        let alias = alias.into();
        if !valid_source_alias(&alias) {
            return Err(OverrideSyntaxError::InvalidSourceAlias);
        }
        Ok(Self {
            source: source.into(),
            alias,
        })
    }
}

/// One typed, canonical-only declaration from `.decided/corpus.md`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct OverrideDeclaration {
    pub parent: QualifiedCanonicalId,
    pub replacement: CanonicalId,
    pub rationale: CanonicalId,
}

impl OverrideDeclaration {
    pub fn parse(
        parent: &str,
        replacement: &str,
        rationale: &str,
    ) -> Result<Self, OverrideSyntaxError> {
        Ok(Self {
            parent: QualifiedCanonicalId::parse(parent)?,
            replacement: CanonicalId::new(replacement)?,
            rationale: CanonicalId::new(rationale)?,
        })
    }
}

/// Why an override declaration did not become effective.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum InvalidOverrideReason {
    ParentAliasMismatch,
    DuplicateParent,
    ParentNotFound,
    ParentAmbiguous,
    ReplacementNotFound,
    ReplacementAmbiguous,
    ReplacementNotLocal,
    Chained,
    TypeMismatch,
    RationaleNotFound,
    RationaleAmbiguous,
    RationaleNotLocal,
    RationaleNotDecision,
    RationaleNotLive,
}

impl InvalidOverrideReason {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ParentAliasMismatch => "parent-alias-mismatch",
            Self::DuplicateParent => "duplicate-parent",
            Self::ParentNotFound => "parent-not-found",
            Self::ParentAmbiguous => "parent-ambiguous",
            Self::ReplacementNotFound => "replacement-not-found",
            Self::ReplacementAmbiguous => "replacement-ambiguous",
            Self::ReplacementNotLocal => "replacement-not-local",
            Self::Chained => "chained",
            Self::TypeMismatch => "type-mismatch",
            Self::RationaleNotFound => "rationale-not-found",
            Self::RationaleAmbiguous => "rationale-ambiguous",
            Self::RationaleNotLocal => "rationale-not-local",
            Self::RationaleNotDecision => "rationale-not-decision",
            Self::RationaleNotLive => "rationale-not-live",
        }
    }
}

/// One deterministic composition finding. These are kept separate from the
/// released path-only relationship finding model until federation activates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompositionFinding {
    pub code: &'static str,
    pub message: String,
    pub reason: Option<InvalidOverrideReason>,
    pub artifacts: Vec<ArtifactKey>,
    pub paths: Vec<ArtifactPath>,
}

/// A validated policy redirect. All three endpoints are stable artifact keys.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatedOverride {
    pub declaration: OverrideDeclaration,
    pub parent: ArtifactKey,
    pub replacement: ArtifactKey,
    pub rationale: ArtifactKey,
}

/// Exact lookup failure against the composed effective view.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LookupError {
    NotFound,
    Ambiguous(Vec<ArtifactKey>),
    InvalidQualifiedReference,
    QualifiedCanonicalRequired,
}

/// The one composed read model. `items` owns the catalog exactly once; local
/// and effective corpora are stable ordered projections over it.
pub struct ComposedCorpus {
    items: Vec<CorpusItem>,
    local: Vec<usize>,
    effective: Vec<usize>,
    parent: Option<ParentIdentity>,
    overrides: Vec<ValidatedOverride>,
    findings: Vec<CompositionFinding>,
    catalog_rows: Vec<ValidationRow>,
    effective_rows: Vec<ValidationRow>,
    resolution_index: ResolutionIndex,
    item_by_key: HashMap<ArtifactKey, usize>,
    captured_content: HashMap<ArtifactKey, Vec<u8>>,
}

impl ComposedCorpus {
    /// A local-only composition useful to consumers adopting the central model
    /// before manifest activation.
    pub fn local(mut items: Vec<CorpusItem>) -> Self {
        items.sort_by(stable_item_order);
        Self::build(items, None, Vec::new(), HashMap::new())
    }

    /// Compose one writable child with one already-verified read-only parent.
    pub fn compose(
        mut local: Vec<CorpusItem>,
        mut inherited: Vec<CorpusItem>,
        parent: ParentIdentity,
        overrides: Vec<OverrideDeclaration>,
    ) -> Self {
        local.append(&mut inherited);
        local.sort_by(stable_item_order);
        Self::build(local, Some(parent), overrides, HashMap::new())
    }

    /// Compose from verification-time snapshots. Captured bytes are owned by
    /// this read model and served by stable key, so a consumer never reopens a
    /// mutable parent path after digest verification.
    pub fn compose_with_content(
        mut local: Vec<CorpusItem>,
        mut inherited: Vec<CorpusItem>,
        parent: ParentIdentity,
        overrides: Vec<OverrideDeclaration>,
        captured_content: impl IntoIterator<Item = (ArtifactKey, Vec<u8>)>,
    ) -> Self {
        local.append(&mut inherited);
        local.sort_by(stable_item_order);
        Self::build(
            local,
            Some(parent),
            overrides,
            captured_content.into_iter().collect(),
        )
    }

    fn build(
        items: Vec<CorpusItem>,
        parent: Option<ParentIdentity>,
        mut declarations: Vec<OverrideDeclaration>,
        mut captured_content: HashMap<ArtifactKey, Vec<u8>>,
    ) -> Self {
        let local: Vec<usize> = items
            .iter()
            .enumerate()
            .filter_map(|(index, item)| (item.origin.layer == Layer::Local).then_some(index))
            .collect();
        let inherited: Vec<usize> = items
            .iter()
            .enumerate()
            .filter_map(|(index, item)| (item.origin.layer == Layer::Inherited).then_some(index))
            .collect();

        let local_canonical = canonical_index(&items, &local);
        let inherited_canonical = canonical_index(&items, &inherited);
        declarations.sort();

        let parent_counts = declarations
            .iter()
            .fold(BTreeMap::new(), |mut counts, declaration| {
                let key = (
                    declaration.parent.alias().to_string(),
                    py_casefold(declaration.parent.canonical_id().as_str()),
                );
                *counts.entry(key).or_insert(0usize) += 1;
                counts
            });
        let declared_parent_ids: BTreeSet<String> = declarations
            .iter()
            .map(|declaration| py_casefold(declaration.parent.canonical_id().as_str()))
            .collect();

        let mut findings = Vec::new();
        let mut valid = Vec::new();
        for declaration in declarations {
            match validate_override(
                &declaration,
                parent.as_ref(),
                &items,
                &local_canonical,
                &inherited_canonical,
                &parent_counts,
                &declared_parent_ids,
            ) {
                Ok(validated) => valid.push(validated),
                Err((reason, item_indices)) => findings.push(invalid_override_finding(
                    &declaration,
                    reason,
                    &items,
                    &item_indices,
                )),
            }
        }

        valid.sort_by(|left, right| left.declaration.cmp(&right.declaration));
        let cleared_collisions: BTreeSet<(ArtifactKey, ArtifactKey)> = valid
            .iter()
            .filter(|mapping| {
                py_casefold(&mapping.parent.canonical_id)
                    == py_casefold(&mapping.replacement.canonical_id)
            })
            .map(|mapping| (mapping.parent.clone(), mapping.replacement.clone()))
            .collect();
        findings.extend(collision_findings(
            &items,
            &local_canonical,
            &inherited_canonical,
            &cleared_collisions,
        ));
        findings.sort_by(finding_order);

        let overridden: BTreeSet<ArtifactKey> =
            valid.iter().map(|mapping| mapping.parent.clone()).collect();
        let effective: Vec<usize> = items
            .iter()
            .enumerate()
            .filter_map(|(index, item)| (!overridden.contains(&item.key)).then_some(index))
            .collect();
        let catalog_rows: Vec<ValidationRow> = items.iter().map(validation_row_from_item).collect();
        let effective_rows: Vec<ValidationRow> = effective
            .iter()
            .map(|index| catalog_rows[*index].clone())
            .collect();
        let resolution_index =
            composed_resolution_index(&catalog_rows, &effective_rows, parent.as_ref(), &valid);
        let item_by_key: HashMap<ArtifactKey, usize> = items
            .iter()
            .enumerate()
            .map(|(index, item)| (item.key.clone(), index))
            .collect();
        captured_content.retain(|key, _| item_by_key.contains_key(key));

        Self {
            items,
            local,
            effective,
            parent,
            overrides: valid,
            findings,
            catalog_rows,
            effective_rows,
            resolution_index,
            item_by_key,
            captured_content,
        }
    }

    pub fn local_items(&self) -> impl ExactSizeIterator<Item = &CorpusItem> {
        self.local.iter().map(|index| &self.items[*index])
    }

    pub fn catalog(&self) -> impl ExactSizeIterator<Item = &CorpusItem> {
        self.items.iter()
    }

    pub fn effective(&self) -> impl ExactSizeIterator<Item = &CorpusItem> {
        self.effective.iter().map(|index| &self.items[*index])
    }

    pub fn parent(&self) -> Option<&ParentIdentity> {
        self.parent.as_ref()
    }

    pub fn overrides(&self) -> &[ValidatedOverride] {
        &self.overrides
    }

    pub fn findings(&self) -> &[CompositionFinding] {
        &self.findings
    }

    pub fn is_overridden(&self, key: &ArtifactKey) -> bool {
        self.overrides.iter().any(|mapping| &mapping.parent == key)
    }

    pub fn item(&self, key: &ArtifactKey) -> Option<&CorpusItem> {
        self.item_by_key.get(key).map(|index| &self.items[*index])
    }

    /// Exact verification-time Markdown bytes, when the composition was built
    /// from captured snapshots. Runtime filesystem locators remain available
    /// on `item` for operations that explicitly need physical provenance.
    pub fn content(&self, key: &ArtifactKey) -> Option<&[u8]> {
        self.captured_content.get(key).map(Vec::as_slice)
    }

    /// Resolve against the effective unqualified view, or the retained parent
    /// catalog when the reference is explicitly qualified.
    pub fn resolve(&self, reference: &str) -> Result<&CorpusItem, LookupError> {
        if reference.contains("::") {
            self.validate_qualified_reference(reference)?;
        }
        let candidates = self.resolution_index.get_reference(reference);
        match candidates {
            [] => Err(LookupError::NotFound),
            [candidate] => self.item(&candidate.key).ok_or(LookupError::NotFound),
            many => Err(LookupError::Ambiguous(
                many.iter().map(|candidate| candidate.key.clone()).collect(),
            )),
        }
    }

    fn validate_qualified_reference(&self, reference: &str) -> Result<(), LookupError> {
        let Some((alias, canonical_id)) = reference.split_once("::") else {
            return Err(LookupError::InvalidQualifiedReference);
        };
        if canonical_id.is_empty() || canonical_id.contains("::") {
            return Err(LookupError::InvalidQualifiedReference);
        }
        let Some(parent) = &self.parent else {
            return Err(LookupError::NotFound);
        };
        if alias != parent.alias {
            return Err(LookupError::NotFound);
        }
        let canonical_fold = py_casefold(canonical_id);
        let canonical_exists = self.items.iter().any(|item| {
            item.origin.layer == Layer::Inherited
                && item.origin.source == parent.source
                && py_casefold(&item.key.canonical_id) == canonical_fold
        });
        if canonical_exists {
            return Ok(());
        }
        let alias_exists = self.items.iter().any(|item| {
            item.origin.layer == Layer::Inherited
                && item.origin.source == parent.source
                && crate::identity::artifact_identifiers(&item.artifact, item.spec, &item.path)
                    .iter()
                    .any(|identifier| py_casefold(identifier) == canonical_fold)
        });
        if alias_exists {
            Err(LookupError::QualifiedCanonicalRequired)
        } else {
            Err(LookupError::NotFound)
        }
    }

    /// Resolve all effective declared edges through the same index as exact
    /// lookup, retaining qualified access to overridden parent history.
    pub fn relationships(&self) -> Vec<Relationship> {
        resolve_relationships(&self.effective_rows, &self.resolution_index)
    }

    /// Resolve declared edges for every retained catalog record, including an
    /// overridden parent's immutable history. Export uses this projection;
    /// live reads and enforcement continue to use `relationships`.
    pub fn catalog_relationships(&self) -> Vec<Relationship> {
        resolve_relationships(&self.catalog_rows, &self.resolution_index)
    }

    /// Run the existing relationship validator over source-aware keys. The
    /// child repository root is intentionally supplied here so inherited
    /// filesystem scope is checked against child code.
    pub fn validate_relationships(
        &self,
        child_directory: &str,
        recursive: bool,
    ) -> RelationshipValidation {
        validation_from_rows_with_index(
            child_directory,
            &self.effective_rows,
            &self.catalog_rows,
            recursive,
            &self.resolution_index,
            false,
        )
    }
}

fn valid_source_alias(alias: &str) -> bool {
    let mut characters = alias.chars();
    let Some(first) = characters.next() else {
        return false;
    };
    first.is_ascii_lowercase()
        && characters.all(|character| {
            character.is_ascii_lowercase()
                || character.is_ascii_digit()
                || matches!(character, '-' | '_' | '.')
        })
}

fn stable_item_order(left: &CorpusItem, right: &CorpusItem) -> std::cmp::Ordering {
    left.artifact_path
        .cmp(&right.artifact_path)
        .then_with(|| left.key.cmp(&right.key))
}

fn canonical_index(items: &[CorpusItem], indices: &[usize]) -> BTreeMap<String, Vec<usize>> {
    let mut index: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for item_index in indices {
        index
            .entry(py_casefold(&items[*item_index].key.canonical_id))
            .or_default()
            .push(*item_index);
    }
    index
}

#[allow(clippy::too_many_arguments)]
fn validate_override(
    declaration: &OverrideDeclaration,
    parent: Option<&ParentIdentity>,
    items: &[CorpusItem],
    local_canonical: &BTreeMap<String, Vec<usize>>,
    inherited_canonical: &BTreeMap<String, Vec<usize>>,
    parent_counts: &BTreeMap<(String, String), usize>,
    declared_parent_ids: &BTreeSet<String>,
) -> Result<ValidatedOverride, (InvalidOverrideReason, Vec<usize>)> {
    let parent_id = py_casefold(declaration.parent.canonical_id().as_str());
    let replacement_id = py_casefold(declaration.replacement.as_str());
    let rationale_id = py_casefold(declaration.rationale.as_str());
    let Some(parent_identity) = parent else {
        return Err((InvalidOverrideReason::ParentNotFound, Vec::new()));
    };
    if declaration.parent.alias() != parent_identity.alias {
        return Err((InvalidOverrideReason::ParentAliasMismatch, Vec::new()));
    }
    if parent_counts
        .get(&(declaration.parent.alias().to_string(), parent_id.clone()))
        .copied()
        .unwrap_or_default()
        > 1
    {
        return Err((InvalidOverrideReason::DuplicateParent, Vec::new()));
    }

    let parent_matches: Vec<usize> = inherited_canonical
        .get(&parent_id)
        .into_iter()
        .flatten()
        .copied()
        .filter(|index| items[*index].origin.source == parent_identity.source)
        .collect();
    let parent_index = match parent_matches.as_slice() {
        [] => return Err((InvalidOverrideReason::ParentNotFound, Vec::new())),
        [index] => *index,
        many => return Err((InvalidOverrideReason::ParentAmbiguous, many.to_vec())),
    };

    let replacement_matches = local_canonical
        .get(&replacement_id)
        .cloned()
        .unwrap_or_default();
    let replacement_index = match replacement_matches.as_slice() {
        [] if inherited_canonical.contains_key(&replacement_id) => {
            return Err((
                InvalidOverrideReason::ReplacementNotLocal,
                inherited_canonical[&replacement_id].clone(),
            ));
        }
        [] => return Err((InvalidOverrideReason::ReplacementNotFound, Vec::new())),
        [index] => *index,
        many => return Err((InvalidOverrideReason::ReplacementAmbiguous, many.to_vec())),
    };
    if replacement_id != parent_id && declared_parent_ids.contains(&replacement_id) {
        return Err((
            InvalidOverrideReason::Chained,
            vec![parent_index, replacement_index],
        ));
    }

    let parent_type = items[parent_index].spec.map(|spec| spec.name.as_str());
    let replacement_type = items[replacement_index].spec.map(|spec| spec.name.as_str());
    if parent_type.is_none() || parent_type != replacement_type {
        return Err((
            InvalidOverrideReason::TypeMismatch,
            vec![parent_index, replacement_index],
        ));
    }

    let rationale_matches = local_canonical
        .get(&rationale_id)
        .cloned()
        .unwrap_or_default();
    let rationale_index = match rationale_matches.as_slice() {
        [] if inherited_canonical.contains_key(&rationale_id) => {
            return Err((
                InvalidOverrideReason::RationaleNotLocal,
                inherited_canonical[&rationale_id].clone(),
            ));
        }
        [] => return Err((InvalidOverrideReason::RationaleNotFound, Vec::new())),
        [index] => *index,
        many => return Err((InvalidOverrideReason::RationaleAmbiguous, many.to_vec())),
    };
    if items[rationale_index].spec.map(|spec| spec.name.as_str()) != Some("decision") {
        return Err((
            InvalidOverrideReason::RationaleNotDecision,
            vec![rationale_index],
        ));
    }
    if !is_live_decision(&items[rationale_index].artifact) {
        return Err((
            InvalidOverrideReason::RationaleNotLive,
            vec![rationale_index],
        ));
    }

    Ok(ValidatedOverride {
        declaration: declaration.clone(),
        parent: items[parent_index].key.clone(),
        replacement: items[replacement_index].key.clone(),
        rationale: items[rationale_index].key.clone(),
    })
}

fn invalid_override_finding(
    declaration: &OverrideDeclaration,
    reason: InvalidOverrideReason,
    items: &[CorpusItem],
    item_indices: &[usize],
) -> CompositionFinding {
    let mut ordered: Vec<usize> = item_indices.to_vec();
    ordered.sort_by(|left, right| stable_item_order(&items[*left], &items[*right]));
    ordered.dedup();
    CompositionFinding {
        code: FINDING_INVALID_OVERRIDE,
        message: format!(
            "override {} -> {} ({}) is invalid: {}",
            declaration.parent,
            declaration.replacement,
            declaration.rationale,
            reason.as_str()
        ),
        reason: Some(reason),
        artifacts: ordered
            .iter()
            .map(|index| items[*index].key.clone())
            .collect(),
        paths: ordered
            .iter()
            .map(|index| items[*index].artifact_path.clone())
            .collect(),
    }
}

fn collision_findings(
    items: &[CorpusItem],
    local: &BTreeMap<String, Vec<usize>>,
    inherited: &BTreeMap<String, Vec<usize>>,
    cleared: &BTreeSet<(ArtifactKey, ArtifactKey)>,
) -> Vec<CompositionFinding> {
    let mut findings = Vec::new();
    for (canonical_fold, local_indices) in local {
        let Some(parent_indices) = inherited.get(canonical_fold) else {
            continue;
        };
        let fully_cleared = local_indices.len() == 1
            && parent_indices.len() == 1
            && cleared.contains(&(
                items[parent_indices[0]].key.clone(),
                items[local_indices[0]].key.clone(),
            ));
        if fully_cleared {
            continue;
        }
        let mut indices: Vec<usize> = local_indices
            .iter()
            .chain(parent_indices)
            .copied()
            .collect();
        indices.sort_by(|left, right| stable_item_order(&items[*left], &items[*right]));
        let display_id = indices
            .first()
            .map(|index| items[*index].key.canonical_id.as_str())
            .unwrap_or(canonical_fold);
        findings.push(CompositionFinding {
            code: FINDING_CANONICAL_COLLISION,
            message: format!(
                "canonical id {display_id} occurs in both local and inherited corpora"
            ),
            reason: None,
            artifacts: indices
                .iter()
                .map(|index| items[*index].key.clone())
                .collect(),
            paths: indices
                .iter()
                .map(|index| items[*index].artifact_path.clone())
                .collect(),
        });
    }
    findings
}

fn finding_order(left: &CompositionFinding, right: &CompositionFinding) -> std::cmp::Ordering {
    left.code
        .cmp(right.code)
        .then_with(|| left.paths.cmp(&right.paths))
        .then_with(|| left.artifacts.cmp(&right.artifacts))
        .then_with(|| left.reason.cmp(&right.reason))
        .then_with(|| left.message.cmp(&right.message))
}

fn composed_resolution_index(
    catalog_rows: &[ValidationRow],
    effective_rows: &[ValidationRow],
    parent: Option<&ParentIdentity>,
    overrides: &[ValidatedOverride],
) -> ResolutionIndex {
    let mut index = resolution_index_from_rows(effective_rows);
    let rows_by_key: HashMap<&ArtifactKey, &ValidationRow> =
        catalog_rows.iter().map(|row| (&row.key, row)).collect();

    if let Some(parent) = parent {
        for row in catalog_rows.iter().filter(|row| {
            row.origin.layer == Layer::Inherited && row.origin.source == parent.source
        }) {
            let qualified = format!("{}::{}", parent.alias, row.canonical_id);
            index.insert(
                ResolutionIndex::reference_key(&qualified),
                ResolutionCandidate::from_row(row, qualified),
            );
        }
    }
    for mapping in overrides {
        let Some(replacement) = rows_by_key.get(&mapping.replacement) else {
            continue;
        };
        index.insert(
            py_casefold(&mapping.parent.canonical_id),
            ResolutionCandidate::from_row(replacement, mapping.parent.canonical_id.clone()),
        );
    }
    index
}
