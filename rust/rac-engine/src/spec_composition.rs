//! Effective-registry composition across a verified federation (ADR-150).
//!
//! A child inherits its parents' admitted bundle types. The effective registry
//! of a corpus is the built-ins in registry order, then its own admitted
//! elements in bundle order, then, for each direct parent in the canonical
//! order the manifest loader fixes, that parent's effective registry beyond
//! the built-ins, recursively. A name already present is skipped when the
//! content is identical; two different contents for one name stop the
//! composition unless the composing corpus records a Decision-backed override
//! under `artifact_types.overrides`.
//!
//! The composer runs from the two places the engine already composes a
//! closure (`graph_federated_corpus` for version 2, `federated_corpus` for
//! version 1) so the CLI and `decided-mcp` cannot disagree, and it installs
//! its result in the process slot `spec` owns before any file is classified.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use crate::corpus::Layer;
use crate::relationships::CorpusItem;
use crate::spec::{
    self, AppliedOverride, ArtifactSpec, Registry, SourceSpecBundle, SpecBundleError, SpecStanza,
    OVERRIDE_PREFER_LOCAL,
};

/// One source of a verified closure as the composer sees it.
#[derive(Debug, Clone)]
pub struct SourceSpecInput {
    pub source: String,
    pub layer: Layer,
    /// The directory holding the source's `.decided/config.yaml`.
    pub repository_root: PathBuf,
    /// The source's captured governing config bytes.
    pub config_bytes: Vec<u8>,
    /// Direct parents, in the canonical order the manifest loader fixes.
    pub parents: Vec<String>,
}

/// One effective element and every source that declared exactly this content.
#[derive(Debug, Clone)]
struct Element {
    spec: ArtifactSpec,
    origins: Vec<String>,
}

impl Element {
    fn declared_by(&self, source: &str) -> bool {
        self.origins.iter().any(|origin| origin == source)
    }
}

/// A source's stanza and, when it pins a bundle, the verified bundle bytes.
struct ParsedSource {
    stanza: SpecStanza,
    bundle: Option<(PathBuf, Vec<u8>)>,
}

/// Compose the effective registry of `root_source` over `nodes` and install
/// it as the process registry. `Ok(None)` when no source declares a stanza:
/// the embedded registry is installed and nothing here leaves a trace.
///
/// Every pinned bundle's bytes are re-verified against its pin before the
/// memo is consulted, so a bundle edited without a re-pin fails closed on the
/// next composition rather than serving the previous registry.
pub fn install_effective_registry(
    root_source: &str,
    nodes: &[SourceSpecInput],
) -> Result<Option<&'static Registry>, SpecBundleError> {
    let mut parsed: BTreeMap<&str, ParsedSource> = BTreeMap::new();
    for node in nodes {
        let stanza = stanza_for(node)?;
        let bundle = match &stanza.pin {
            Some(pin) => Some(in_source(
                node,
                spec::read_verified_bundle(&node.repository_root, pin),
            )?),
            None => None,
        };
        parsed.insert(node.source.as_str(), ParsedSource { stanza, bundle });
    }
    if parsed
        .values()
        .all(|source| source.stanza.pin.is_none() && source.stanza.overrides.is_empty())
    {
        spec::install(None);
        return Ok(None);
    }
    let frames: Vec<(&str, &[u8])> = nodes
        .iter()
        .map(|node| (node.source.as_str(), node.config_bytes.as_slice()))
        .collect();
    let key = spec::registry_key(&frames);
    let registry = spec::registry_for_key(&key, || compose(root_source, nodes, &parsed))?;
    spec::install(Some(registry));
    Ok(Some(registry))
}

/// Verify every pinned bundle in the closure against its pin without
/// composing, and report whether any source declares an `artifact_types`
/// stanza. Cache layers call this on paths that reuse a generation without
/// recomposing, so a bundle edited without a re-pin still fails closed there.
pub fn verify_pinned_bundles(nodes: &[SourceSpecInput]) -> Result<bool, SpecBundleError> {
    let mut any_stanza = false;
    for node in nodes {
        let stanza = stanza_for(node)?;
        if let Some(pin) = &stanza.pin {
            in_source(node, spec::read_verified_bundle(&node.repository_root, pin))?;
        }
        any_stanza |= stanza.pin.is_some() || !stanza.overrides.is_empty();
    }
    Ok(any_stanza)
}

/// Check every applied override's rationale against the parsed closure: it
/// must resolve to exactly one live local Decision of the declaring source.
pub fn verify_override_rationales<'a>(
    registry: &Registry,
    items: impl IntoIterator<Item = &'a CorpusItem>,
) -> Result<(), SpecBundleError> {
    let items: Vec<&CorpusItem> = items.into_iter().collect();
    for applied in registry.overrides() {
        let matches: Vec<&CorpusItem> = items
            .iter()
            .copied()
            .filter(|item| {
                item.origin.source == applied.owner && item.key.canonical_id == applied.rationale
            })
            .collect();
        let reason = match matches.as_slice() {
            [] => Some(format!(
                "rationale '{}' does not resolve to a local artifact of '{}'",
                applied.rationale, applied.owner
            )),
            [one] => {
                if one.spec.map(|spec| spec.name.as_str()) != Some("decision") {
                    Some(format!(
                        "rationale '{}' is not a Decision",
                        applied.rationale
                    ))
                } else if !crate::resolve::is_live_decision(&one.artifact) {
                    Some(format!(
                        "rationale '{}' is not a live Decision (Accepted and not retired)",
                        applied.rationale
                    ))
                } else {
                    None
                }
            }
            many => Some(format!(
                "rationale '{}' is ambiguous: {} local artifacts of '{}' carry it",
                applied.rationale,
                many.len(),
                applied.owner
            )),
        };
        if let Some(reason) = reason {
            return Err(SpecBundleError::InvalidOverride {
                owner: applied.owner.clone(),
                name: applied.name.clone(),
                reason,
            });
        }
    }
    Ok(())
}

/// Attribute a bundle or config failure to an inherited source.
fn in_source<T>(
    node: &SourceSpecInput,
    result: Result<T, SpecBundleError>,
) -> Result<T, SpecBundleError> {
    match (node.layer, result) {
        (Layer::Inherited, Err(error)) => Err(SpecBundleError::InSource {
            source: node.source.clone(),
            error: Box::new(error),
        }),
        (_, result) => result,
    }
}

/// The source's stanza, read exactly as `sync_registry` reads the local one:
/// an unreadable config that mentions `artifact_types` is a hard error.
fn stanza_for(node: &SourceSpecInput) -> Result<SpecStanza, SpecBundleError> {
    let Some(config) = in_source(node, spec::parse_config_bytes(&node.config_bytes))? else {
        return Ok(SpecStanza::default());
    };
    in_source(node, spec::spec_stanza_from_config(&config)).map(Option::unwrap_or_default)
}

struct Composer<'a> {
    nodes: BTreeMap<&'a str, &'a SourceSpecInput>,
    parsed: &'a BTreeMap<&'a str, ParsedSource>,
    memo: BTreeMap<String, Vec<Element>>,
    sources: Vec<SourceSpecBundle>,
    overrides: Vec<AppliedOverride>,
}

fn compose(
    root_source: &str,
    nodes: &[SourceSpecInput],
    parsed: &BTreeMap<&str, ParsedSource>,
) -> Result<Registry, SpecBundleError> {
    let mut composer = Composer {
        nodes: nodes
            .iter()
            .map(|node| (node.source.as_str(), node))
            .collect(),
        parsed,
        memo: BTreeMap::new(),
        sources: Vec::new(),
        overrides: Vec::new(),
    };
    let effective = composer.effective(root_source)?;
    let local_bundle = composer
        .sources
        .iter()
        .find(|source| source.source == root_source && source.layer == Layer::Local)
        .map(|source| source.bundle.clone());
    let mut specs = spec::builtin_specs().to_vec();
    specs.extend(effective.into_iter().map(|element| element.spec));
    let local_config_digest = composer
        .nodes
        .get(root_source)
        .map(|node| crate::sha256::hexdigest(&node.config_bytes))
        .unwrap_or_default();
    Ok(Registry::from_parts(
        specs,
        local_bundle,
        composer.sources,
        composer.overrides,
        true,
        local_config_digest,
    ))
}

impl<'a> Composer<'a> {
    fn node(&self, source: &str) -> Result<&'a SourceSpecInput, SpecBundleError> {
        self.nodes.get(source).copied().ok_or_else(|| {
            SpecBundleError::Config(format!(
                "federation source '{source}' is declared as a parent but was not verified"
            ))
        })
    }

    /// Every transitive parent of `source`: the sources whose elements are in
    /// its inherited view.
    fn ancestors(&self, source: &str) -> Result<BTreeSet<String>, SpecBundleError> {
        let mut seen = BTreeSet::new();
        let mut pending: Vec<String> = self.node(source)?.parents.clone();
        while let Some(parent) = pending.pop() {
            if seen.insert(parent.clone()) {
                pending.extend(self.node(&parent)?.parents.iter().cloned());
            }
        }
        Ok(seen)
    }

    fn effective(&mut self, source: &str) -> Result<Vec<Element>, SpecBundleError> {
        if let Some(done) = self.memo.get(source) {
            return Ok(done.clone());
        }
        let node = self.node(source)?;
        let parsed = self
            .parsed
            .get(source)
            .expect("every verified source was parsed");

        // Candidates: own admitted elements first, then each parent's
        // effective elements in canonical order.
        let mut candidates: Vec<Element> = Vec::new();
        if let (Some(pin), Some((path, bytes))) = (&parsed.stanza.pin, &parsed.bundle) {
            let registry = in_source(node, spec::admit_bundle(path, bytes, pin))?;
            let bundle = registry
                .bundle()
                .cloned()
                .expect("an admitted bundle carries its report");
            self.sources.push(SourceSpecBundle {
                source: source.to_string(),
                layer: node.layer,
                bundle,
            });
            let builtin = spec::builtin_specs().len();
            candidates.extend(
                registry.specs()[builtin..]
                    .iter()
                    .cloned()
                    .map(|spec| Element {
                        spec,
                        origins: vec![source.to_string()],
                    }),
            );
        }
        for parent in &node.parents {
            candidates.extend(self.effective(parent)?);
        }

        // Group by name in first-appearance order; identical content is one
        // element declared by several sources.
        let mut order: Vec<String> = Vec::new();
        let mut groups: BTreeMap<String, Vec<Element>> = BTreeMap::new();
        for candidate in candidates {
            let group = groups
                .entry(candidate.spec.name.clone())
                .or_insert_with(|| {
                    order.push(candidate.spec.name.clone());
                    Vec::new()
                });
            match group
                .iter_mut()
                .find(|element| element.spec == candidate.spec)
            {
                Some(same) => {
                    for origin in candidate.origins {
                        if !same.declared_by(&origin) {
                            same.origins.push(origin);
                        }
                    }
                }
                None => group.push(candidate),
            }
        }
        let colliding: BTreeSet<&str> = groups
            .iter()
            .filter(|(_, group)| group.len() > 1)
            .map(|(name, _)| name.as_str())
            .collect();

        // Every override must name a real collision and a source in view.
        let overrides = &parsed.stanza.overrides;
        let ancestors = self.ancestors(source)?;
        for declared in overrides {
            let invalid = |reason: String| SpecBundleError::InvalidOverride {
                owner: source.to_string(),
                name: declared.name.clone(),
                reason,
            };
            if declared.prefer != OVERRIDE_PREFER_LOCAL && !ancestors.contains(&declared.prefer) {
                return Err(invalid(format!(
                    "prefer '{}' is not a parent in the inherited view of '{source}'",
                    declared.prefer
                )));
            }
            if !colliding.contains(declared.name.as_str()) {
                return Err(invalid(
                    "the artifact type does not collide; only a name two sources declare with \
                     different content may be overridden"
                        .to_string(),
                ));
            }
        }

        let mut result = Vec::with_capacity(order.len());
        for name in order {
            let mut group = groups.remove(&name).expect("grouped by name");
            if group.len() == 1 {
                result.push(group.remove(0));
                continue;
            }
            let Some(declared) = overrides.iter().find(|o| o.name == name) else {
                return Err(SpecBundleError::TypeConflict {
                    owner: source.to_string(),
                    name,
                    sources: group
                        .iter()
                        .flat_map(|element| element.origins.iter().cloned())
                        .collect(),
                });
            };
            let preferred = if declared.prefer == OVERRIDE_PREFER_LOCAL {
                source
            } else {
                declared.prefer.as_str()
            };
            let Some(winner) = group.iter().find(|element| element.declared_by(preferred)) else {
                return Err(SpecBundleError::InvalidOverride {
                    owner: source.to_string(),
                    name,
                    reason: format!(
                        "prefer '{}' declares no candidate; the colliding declarations are by {}",
                        declared.prefer,
                        group
                            .iter()
                            .flat_map(|element| element.origins.iter())
                            .map(|origin| format!("'{origin}'"))
                            .collect::<Vec<_>>()
                            .join(" and ")
                    ),
                });
            };
            result.push(winner.clone());
        }
        for declared in overrides {
            self.overrides.push(AppliedOverride {
                owner: source.to_string(),
                name: declared.name.clone(),
                prefer: declared.prefer.clone(),
                rationale: declared.rationale.clone(),
            });
        }
        self.memo.insert(source.to_string(), result.clone());
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spec::{BundlePin, TypeOverride};
    use std::path::Path;

    const RUNBOOK: &str = r#"{"name":"runbook","display":"Runbook","required":["purpose","steps"],"recommended":[],"optional":[],"metadata":{"status":["Draft","Active"]},"retired_status":[],"descriptions":{},"guidance":{},"synonyms":{},"id_field":null,"starter_bodies":{},"okf_type":"Runbook"}"#;
    const RUNBOOK_ALT: &str = r#"{"name":"runbook","display":"Run Book","required":["purpose"],"recommended":[],"optional":[],"metadata":{},"retired_status":[],"descriptions":{},"guidance":{},"synonyms":{},"id_field":null,"starter_bodies":{}}"#;
    const POLICY: &str = r#"{"name":"policy","display":"Policy","required":["scope","rules"],"recommended":[],"optional":[],"metadata":{},"retired_status":[],"descriptions":{},"guidance":{},"synonyms":{},"id_field":null,"starter_bodies":{}}"#;

    fn scratch(tag: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "asdecided-spec-composition-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(root.join(".decided")).unwrap();
        root
    }

    fn pin(root: &Path, elements: &[&str]) -> BundlePin {
        let json = format!("{{\"artifact_specs\":[{}]}}", elements.join(","));
        std::fs::write(root.join(".decided/artifact-specs.json"), &json).unwrap();
        BundlePin {
            path: ".decided/artifact-specs.json".to_string(),
            digest: format!("sha256:{}", crate::sha256::hexdigest(json.as_bytes())),
        }
    }

    fn config(source: &str, pin: Option<&BundlePin>, overrides: &[TypeOverride]) -> Vec<u8> {
        let mut text = format!("repository_key: T\ncorpus:\n  source: {source}\n");
        if pin.is_some() || !overrides.is_empty() {
            text.push_str("artifact_types:\n  version: 1\n");
        }
        if let Some(pin) = pin {
            text.push_str(&format!(
                "  bundle:\n    path: {}\n    digest: {}\n",
                pin.path, pin.digest
            ));
        }
        if !overrides.is_empty() {
            text.push_str("  overrides:\n");
            for o in overrides {
                text.push_str(&format!(
                    "    - name: {}\n      prefer: {}\n      rationale: {}\n",
                    o.name, o.prefer, o.rationale
                ));
            }
        }
        text.into_bytes()
    }

    fn node(
        source: &str,
        layer: Layer,
        root: &Path,
        pin: Option<&BundlePin>,
        overrides: &[TypeOverride],
        parents: &[&str],
    ) -> SourceSpecInput {
        SourceSpecInput {
            source: source.to_string(),
            layer,
            repository_root: root.to_path_buf(),
            config_bytes: config(source, pin, overrides),
            parents: parents.iter().map(|p| p.to_string()).collect(),
        }
    }

    fn names(registry: &Registry) -> Vec<&str> {
        registry
            .specs()
            .iter()
            .skip(spec::builtin_specs().len())
            .map(|s| s.name.as_str())
            .collect()
    }

    fn compose_only(root: &str, nodes: &[SourceSpecInput]) -> Result<Registry, SpecBundleError> {
        let mut parsed = BTreeMap::new();
        for n in nodes {
            let stanza = stanza_for(n)?;
            let bundle = match &stanza.pin {
                Some(pin) => Some(in_source(
                    n,
                    spec::read_verified_bundle(&n.repository_root, pin),
                )?),
                None => None,
            };
            parsed.insert(n.source.as_str(), ParsedSource { stanza, bundle });
        }
        compose(root, nodes, &parsed)
    }

    fn overr(name: &str, prefer: &str) -> TypeOverride {
        TypeOverride {
            name: name.to_string(),
            prefer: prefer.to_string(),
            rationale: "ADR-001".to_string(),
        }
    }

    #[test]
    fn child_inherits_parent_types_after_its_own() {
        let child = scratch("child");
        let parent = scratch("parent");
        let child_pin = pin(&child, &[POLICY]);
        let parent_pin = pin(&parent, &[RUNBOOK]);
        let nodes = [
            node(
                "acme/child",
                Layer::Local,
                &child,
                Some(&child_pin),
                &[],
                &["acme/parent"],
            ),
            node(
                "acme/parent",
                Layer::Inherited,
                &parent,
                Some(&parent_pin),
                &[],
                &[],
            ),
        ];
        let registry = compose_only("acme/child", &nodes).unwrap();
        assert_eq!(names(&registry), ["policy", "runbook"]);
        assert!(registry.is_federated());
        let sources: Vec<(&str, Layer)> = registry
            .sources()
            .iter()
            .map(|s| (s.source.as_str(), s.layer))
            .collect();
        assert_eq!(
            sources,
            [
                ("acme/child", Layer::Local),
                ("acme/parent", Layer::Inherited)
            ]
        );
        assert_eq!(registry.bundle().unwrap().admitted, ["policy"]);
    }

    #[test]
    fn identical_duplicates_are_silent_and_different_content_conflicts() {
        let child = scratch("dup-child");
        let left = scratch("dup-left");
        let right = scratch("dup-right");
        let left_pin = pin(&left, &[RUNBOOK]);
        let right_pin = pin(&right, &[RUNBOOK]);
        let nodes = [
            node(
                "acme/child",
                Layer::Local,
                &child,
                None,
                &[],
                &["acme/left", "acme/right"],
            ),
            node(
                "acme/left",
                Layer::Inherited,
                &left,
                Some(&left_pin),
                &[],
                &[],
            ),
            node(
                "acme/right",
                Layer::Inherited,
                &right,
                Some(&right_pin),
                &[],
                &[],
            ),
        ];
        let registry = compose_only("acme/child", &nodes).unwrap();
        assert_eq!(names(&registry), ["runbook"]);
        assert!(registry.bundle().is_none());

        let right_pin = pin(&right, &[RUNBOOK_ALT]);
        let nodes = [
            node(
                "acme/child",
                Layer::Local,
                &child,
                None,
                &[],
                &["acme/left", "acme/right"],
            ),
            node(
                "acme/left",
                Layer::Inherited,
                &left,
                Some(&left_pin),
                &[],
                &[],
            ),
            node(
                "acme/right",
                Layer::Inherited,
                &right,
                Some(&right_pin),
                &[],
                &[],
            ),
        ];
        let error = compose_only("acme/child", &nodes).unwrap_err();
        assert_eq!(
            error.stable_code(),
            "corpus-federation-artifact-type-conflict"
        );
        assert_eq!(error.source(), Some("acme/child"));
        assert!(
            error.detail().contains("'acme/left' and 'acme/right'"),
            "{error}"
        );
    }

    #[test]
    fn an_override_selects_the_preferred_source_and_binds_descendants() {
        let child = scratch("ov-child");
        let left = scratch("ov-left");
        let right = scratch("ov-right");
        let left_pin = pin(&left, &[RUNBOOK]);
        let right_pin = pin(&right, &[RUNBOOK_ALT]);
        let nodes = [
            node(
                "acme/child",
                Layer::Local,
                &child,
                None,
                &[overr("runbook", "acme/right")],
                &["acme/left", "acme/right"],
            ),
            node(
                "acme/left",
                Layer::Inherited,
                &left,
                Some(&left_pin),
                &[],
                &[],
            ),
            node(
                "acme/right",
                Layer::Inherited,
                &right,
                Some(&right_pin),
                &[],
                &[],
            ),
        ];
        let registry = compose_only("acme/child", &nodes).unwrap();
        assert_eq!(registry.spec_for("runbook").unwrap().display, "Run Book");
        assert_eq!(registry.overrides().len(), 1);
        assert_eq!(registry.overrides()[0].owner, "acme/child");

        // A local declaration wins with `prefer: local`.
        let local_pin = pin(&child, &[RUNBOOK_ALT]);
        let nodes = [
            node(
                "acme/child",
                Layer::Local,
                &child,
                Some(&local_pin),
                &[overr("runbook", "local")],
                &["acme/left"],
            ),
            node(
                "acme/left",
                Layer::Inherited,
                &left,
                Some(&left_pin),
                &[],
                &[],
            ),
        ];
        let registry = compose_only("acme/child", &nodes).unwrap();
        assert_eq!(registry.spec_for("runbook").unwrap().display, "Run Book");
    }

    #[test]
    fn override_defects_are_invalid_overrides() {
        let child = scratch("bad-child");
        let left = scratch("bad-left");
        let right = scratch("bad-right");
        let left_pin = pin(&left, &[RUNBOOK]);
        let right_pin = pin(&right, &[RUNBOOK_ALT]);
        let with = |overrides: &[TypeOverride]| {
            [
                node(
                    "acme/child",
                    Layer::Local,
                    &child,
                    None,
                    overrides,
                    &["acme/left", "acme/right"],
                ),
                node(
                    "acme/left",
                    Layer::Inherited,
                    &left,
                    Some(&left_pin),
                    &[],
                    &[],
                ),
                node(
                    "acme/right",
                    Layer::Inherited,
                    &right,
                    Some(&right_pin),
                    &[],
                    &[],
                ),
            ]
        };
        for (overrides, fragment) in [
            (
                vec![overr("runbook", "acme/other")],
                "not a parent in the inherited view",
            ),
            (vec![overr("runbook", "local")], "declares no candidate"),
            (
                vec![overr("runbook", "acme/left"), overr("policy", "acme/left")],
                "does not collide",
            ),
        ] {
            let error = compose_only("acme/child", &with(&overrides)).unwrap_err();
            assert_eq!(error.stable_code(), "corpus-federation-invalid-override");
            assert!(error.detail().contains(fragment), "{error}");
        }
    }

    #[test]
    fn a_parent_bundle_failure_names_the_parent() {
        let child = scratch("pf-child");
        let parent = scratch("pf-parent");
        let mut parent_pin = pin(&parent, &[RUNBOOK]);
        parent_pin.digest = format!("sha256:{}", "0".repeat(64));
        let nodes = [
            node(
                "acme/child",
                Layer::Local,
                &child,
                None,
                &[],
                &["acme/parent"],
            ),
            node(
                "acme/parent",
                Layer::Inherited,
                &parent,
                Some(&parent_pin),
                &[],
                &[],
            ),
        ];
        let error = compose_only("acme/child", &nodes).unwrap_err();
        assert_eq!(error.stable_code(), "artifact-spec-bundle-digest-mismatch");
        assert_eq!(error.source(), Some("acme/parent"));
        assert!(error
            .detail()
            .starts_with("inherited source 'acme/parent': "));
    }

    #[test]
    fn a_closure_without_any_stanza_is_inert() {
        let child = scratch("inert-child");
        let parent = scratch("inert-parent");
        let nodes = [
            node(
                "acme/child",
                Layer::Local,
                &child,
                None,
                &[],
                &["acme/parent"],
            ),
            node("acme/parent", Layer::Inherited, &parent, None, &[], &[]),
        ];
        assert!(install_effective_registry("acme/child", &nodes)
            .unwrap()
            .is_none());
        assert!(spec::active_registry().is_none());
    }
}
