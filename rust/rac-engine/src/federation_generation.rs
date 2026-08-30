//! Canonical closure-generation hashing for graph federation (ADR-148).
//!
//! This module is deliberately filesystem-free. The verified loader owns
//! capture, containment, limits, topology, and semantic validation; this
//! module turns that already-verified logical closure into the exact portable
//! `sha256-v3:` generation used by derived state and `store/v3`.

use crate::{corpus::ArtifactKey, sha256::Sha256};

pub const GENERATION_DOMAIN: &[u8] = b"asdecided-federation-generation-v3\0";
pub const GRAPH_CONTRACT: &[u8] = b"corpus-federation-graph/v2";
pub const ARTIFACT_SPEC_FINGERPRINT: &[u8] = b"artifact-spec-registry/v1";
pub const RELATIONSHIP_DESCRIPTION_FINGERPRINT: &[u8] = b"relationship-description-registry/v1";
pub const TOKENIZER_RANKING_FINGERPRINT: &[u8] = b"tokenizer-ranking-graph-floor/v1";
pub const DERIVED_SCHEMA_FINGERPRINT: &[u8] = b"federation-derived/v3";
pub const STORE_LAYOUT_FINGERPRINT: &[u8] = b"store/v3";

/// Exact ADR-144 limit block committed into every version-3 generation.
pub const LIMIT_BLOCK: &[u8] = b"manifest-bytes=1048576\n\
config-bytes=1048576\n\
alias-bytes=64\n\
source-bytes=255\n\
path-bytes=4096\n\
path-components=64\n\
path-component-bytes=255\n\
yaml-depth=32\n\
yaml-nodes=16384\n\
direct-parents=32\n\
depth=16\n\
unique-inherited-sources=256\n\
edges=1024\n\
overrides=4096\n\
inherited-files=50000\n\
file-bytes=16777216\n\
logical-bytes=268435456\n\
physical-bytes=536870912\n\
visited-entries=200000\n";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GenerationFile {
    pub relative_path: String,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GenerationNode {
    pub source: String,
    /// Canonical `sha256-v2:` digest, independent of the declared edge pin.
    pub canonical_digest: String,
    pub config_bytes: Vec<u8>,
    pub manifest_bytes: Option<Vec<u8>>,
    pub files: Vec<GenerationFile>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GenerationEdge {
    pub owner_source: String,
    pub target_source: String,
    /// Exact declared `sha256:` or `sha256-v2:` pin text.
    pub declared_pin: String,
    pub alias: String,
    pub root: String,
    pub corpus: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GenerationMapping {
    /// Parents-before-child, source-lexicographic Kahn rank. The rank selects
    /// the ADR-147 total order but is not itself framed into the generation.
    pub owner_rank: usize,
    pub owner_source: String,
    pub target: ArtifactKey,
    pub replacement: ArtifactKey,
    pub rationale: ArtifactKey,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GenerationRedirect {
    pub target: ArtifactKey,
    pub terminal: ArtifactKey,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphGenerationInput {
    pub recursive: bool,
    pub root_source: String,
    /// Stable POSIX path relative to the root repository.
    pub root_corpus_path: String,
    pub root_config_bytes: Vec<u8>,
    pub root_manifest_bytes: Option<Vec<u8>>,
    pub root_files: Vec<GenerationFile>,
    pub inherited_nodes: Vec<GenerationNode>,
    pub edges: Vec<GenerationEdge>,
    pub mappings: Vec<GenerationMapping>,
    pub terminal_redirects: Vec<GenerationRedirect>,
}

/// Compute ADR-148's canonical closure generation.
///
/// Every semantic table is independently sorted, so filesystem discovery,
/// traversal, and manifest parent-list order cannot become precedence. Exact
/// manifest bytes remain committed as required by the accepted contract.
pub fn closure_generation(input: &GraphGenerationInput) -> String {
    let mut hasher = Sha256::new();
    hasher.update(GENERATION_DOMAIN);

    frame(&mut hasher, 0x01, GRAPH_CONTRACT);
    frame(&mut hasher, 0x02, LIMIT_BLOCK);
    frame(
        &mut hasher,
        0x03,
        &[if input.recursive { 0x01 } else { 0x00 }],
    );
    frame(&mut hasher, 0x04, input.root_source.as_bytes());
    frame(&mut hasher, 0x05, input.root_corpus_path.as_bytes());
    frame(&mut hasher, 0x06, &input.root_config_bytes);
    manifest_frames(
        &mut hasher,
        0x07,
        0x08,
        input.root_manifest_bytes.as_deref(),
    );

    for file in sorted_files(&input.root_files) {
        frame(&mut hasher, 0x09, file.relative_path.as_bytes());
        frame(&mut hasher, 0x0a, &file.bytes);
    }

    let mut nodes: Vec<_> = input.inherited_nodes.iter().collect();
    nodes.sort_by(|left, right| {
        (&left.source, &left.canonical_digest).cmp(&(&right.source, &right.canonical_digest))
    });
    for node in nodes {
        frame(&mut hasher, 0x10, node.source.as_bytes());
        frame(&mut hasher, 0x11, node.canonical_digest.as_bytes());
        frame(&mut hasher, 0x12, &node.config_bytes);
        manifest_frames(&mut hasher, 0x13, 0x14, node.manifest_bytes.as_deref());
        for file in sorted_files(&node.files) {
            frame(&mut hasher, 0x15, file.relative_path.as_bytes());
            frame(&mut hasher, 0x16, &file.bytes);
        }
        frame(&mut hasher, 0x17, &[]);
    }

    let mut edges: Vec<_> = input.edges.iter().collect();
    edges.sort_by(|left, right| edge_key(left).cmp(&edge_key(right)));
    for edge in edges {
        frame(&mut hasher, 0x20, edge.owner_source.as_bytes());
        frame(&mut hasher, 0x21, edge.target_source.as_bytes());
        frame(&mut hasher, 0x22, edge.declared_pin.as_bytes());
        frame(&mut hasher, 0x23, edge.alias.as_bytes());
        frame(&mut hasher, 0x24, edge.root.as_bytes());
        frame(&mut hasher, 0x25, edge.corpus.as_bytes());
        frame(&mut hasher, 0x26, &[]);
    }

    let mut mappings: Vec<_> = input.mappings.iter().collect();
    mappings.sort_by(|left, right| mapping_key(left).cmp(&mapping_key(right)));
    for mapping in mappings {
        frame(&mut hasher, 0x30, mapping.owner_source.as_bytes());
        artifact_frames(&mut hasher, 0x31, 0x32, &mapping.target);
        artifact_frames(&mut hasher, 0x33, 0x34, &mapping.replacement);
        artifact_frames(&mut hasher, 0x35, 0x36, &mapping.rationale);
        frame(&mut hasher, 0x37, &[]);
    }

    let mut redirects: Vec<_> = input.terminal_redirects.iter().collect();
    redirects.sort_by(|left, right| {
        (&left.target.source, &left.target.canonical_id)
            .cmp(&(&right.target.source, &right.target.canonical_id))
    });
    for redirect in redirects {
        artifact_frames(&mut hasher, 0x38, 0x39, &redirect.target);
        artifact_frames(&mut hasher, 0x3a, 0x3b, &redirect.terminal);
        frame(&mut hasher, 0x3c, &[]);
    }

    frame(&mut hasher, 0x40, ARTIFACT_SPEC_FINGERPRINT);
    frame(&mut hasher, 0x41, RELATIONSHIP_DESCRIPTION_FINGERPRINT);
    frame(&mut hasher, 0x42, TOKENIZER_RANKING_FINGERPRINT);
    frame(&mut hasher, 0x43, DERIVED_SCHEMA_FINGERPRINT);
    frame(&mut hasher, 0x44, STORE_LAYOUT_FINGERPRINT);

    format!("sha256-v3:{}", hasher.hexdigest())
}

/// Adapt the exact verified closure and its one compiled semantic graph into
/// ADR-148's logical-generation input without reopening either layer.
pub fn generation_input_from_verified(
    verified: &crate::federation::VerifiedFederation,
    recursive: bool,
    composition: &crate::graph_composition::GraphComposition,
) -> GraphGenerationInput {
    GraphGenerationInput {
        recursive,
        root_source: verified.root_source.clone(),
        root_corpus_path: verified.root_corpus_path.clone(),
        root_config_bytes: verified.root_config_bytes.clone(),
        root_manifest_bytes: Some(verified.manifest.bytes.clone()),
        root_files: verified.root_files.iter().map(generation_file).collect(),
        inherited_nodes: verified
            .nodes
            .iter()
            .map(|node| GenerationNode {
                source: node.source.clone(),
                canonical_digest: node.digest.clone(),
                config_bytes: node.config_bytes.clone(),
                manifest_bytes: node.manifest_bytes.clone(),
                files: node.files.iter().map(generation_file).collect(),
            })
            .collect(),
        edges: verified
            .edges
            .iter()
            .map(|edge| GenerationEdge {
                owner_source: edge.owner_source.clone(),
                target_source: edge.target_source.clone(),
                declared_pin: edge.declared_digest.clone(),
                alias: edge.alias.clone(),
                root: edge.root.clone(),
                corpus: edge.corpus.clone(),
            })
            .collect(),
        mappings: composition
            .ordered_overrides()
            .iter()
            .map(|mapping| GenerationMapping {
                owner_rank: mapping.owner_rank,
                owner_source: mapping.owner_source.clone(),
                target: mapping.target.clone(),
                replacement: mapping.replacement.clone(),
                rationale: mapping.rationale.clone(),
            })
            .collect(),
        terminal_redirects: composition
            .terminal_redirects()
            .iter()
            .map(|(target, terminal)| GenerationRedirect {
                target: target.clone(),
                terminal: terminal.clone(),
            })
            .collect(),
    }
}

pub fn closure_generation_from_verified(
    verified: &crate::federation::VerifiedFederation,
    recursive: bool,
    composition: &crate::graph_composition::GraphComposition,
) -> String {
    closure_generation(&generation_input_from_verified(
        verified,
        recursive,
        composition,
    ))
}

fn generation_file(file: &crate::federation::SnapshotFile) -> GenerationFile {
    GenerationFile {
        relative_path: file.relative_path.clone(),
        bytes: file.bytes.clone(),
    }
}

fn frame(hasher: &mut Sha256, tag: u8, payload: &[u8]) {
    hasher.update(&[tag]);
    hasher.update(&(payload.len() as u64).to_be_bytes());
    hasher.update(payload);
}

fn manifest_frames(hasher: &mut Sha256, presence_tag: u8, bytes_tag: u8, manifest: Option<&[u8]>) {
    match manifest {
        Some(bytes) => {
            frame(hasher, presence_tag, &[0x01]);
            frame(hasher, bytes_tag, bytes);
        }
        None => frame(hasher, presence_tag, &[0x00]),
    }
}

fn artifact_frames(hasher: &mut Sha256, source_tag: u8, id_tag: u8, key: &ArtifactKey) {
    frame(hasher, source_tag, key.source.as_bytes());
    frame(hasher, id_tag, key.canonical_id.as_bytes());
}

fn sorted_files(files: &[GenerationFile]) -> Vec<&GenerationFile> {
    let mut sorted: Vec<_> = files.iter().collect();
    sorted.sort_by(|left, right| {
        left.relative_path
            .as_bytes()
            .cmp(right.relative_path.as_bytes())
    });
    sorted
}

fn edge_key(edge: &GenerationEdge) -> (&str, &str, &str, &str, &str, &str) {
    (
        &edge.owner_source,
        &edge.target_source,
        &edge.declared_pin,
        &edge.alias,
        &edge.root,
        &edge.corpus,
    )
}

fn mapping_key(
    mapping: &GenerationMapping,
) -> (usize, &str, &str, &str, &str, &str, &str, &str) {
    (
        mapping.owner_rank,
        &mapping.owner_source,
        &mapping.target.source,
        &mapping.target.canonical_id,
        &mapping.replacement.source,
        &mapping.replacement.canonical_id,
        &mapping.rationale.source,
        &mapping.rationale.canonical_id,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(source: &str, id: &str) -> ArtifactKey {
        ArtifactKey::new(source, id)
    }

    fn fixture() -> GraphGenerationInput {
        GraphGenerationInput {
            recursive: true,
            root_source: "acme/app".into(),
            root_corpus_path: "decisions".into(),
            root_config_bytes: b"corpus:\n  source: acme/app\n".to_vec(),
            root_manifest_bytes: Some(
                b"# Corpus\n\n## inherits\n\n```yaml\nversion: 2\n```\n".to_vec(),
            ),
            root_files: vec![
                GenerationFile {
                    relative_path: "requirements/b.md".into(),
                    bytes: b"B".to_vec(),
                },
                GenerationFile {
                    relative_path: "decisions/a.md".into(),
                    bytes: b"A".to_vec(),
                },
            ],
            inherited_nodes: vec![GenerationNode {
                source: "acme/standards".into(),
                canonical_digest: format!("sha256-v2:{}", "1".repeat(64)),
                config_bytes: b"corpus:\n  source: acme/standards\n".to_vec(),
                manifest_bytes: None,
                files: vec![GenerationFile {
                    relative_path: "decisions/standard.md".into(),
                    bytes: b"standard".to_vec(),
                }],
            }],
            edges: vec![GenerationEdge {
                owner_source: "acme/app".into(),
                target_source: "acme/standards".into(),
                declared_pin: format!("sha256-v2:{}", "1".repeat(64)),
                alias: "standards".into(),
                root: "vendor/standards".into(),
                corpus: "decisions".into(),
            }],
            mappings: vec![GenerationMapping {
                owner_rank: 1,
                owner_source: "acme/app".into(),
                target: key("acme/standards", "STD-0123456789AB"),
                replacement: key("acme/app", "APP-0123456789AB"),
                rationale: key("acme/app", "APP-ABCDEFGHJKMN"),
            }],
            terminal_redirects: vec![GenerationRedirect {
                target: key("acme/standards", "STD-0123456789AB"),
                terminal: key("acme/app", "APP-0123456789AB"),
            }],
        }
    }

    #[test]
    fn generation_has_frozen_known_vector() {
        assert_eq!(
            closure_generation(&fixture()),
            "sha256-v3:2e64f14ddf26d996a5910a844d92ad66e121b1711b9899560c77257134f79c65"
        );
    }

    #[test]
    fn semantic_table_order_does_not_change_generation() {
        let expected = closure_generation(&fixture());
        let mut permuted = fixture();
        permuted.root_files.reverse();
        permuted.inherited_nodes.reverse();
        permuted.edges.reverse();
        permuted.mappings.reverse();
        permuted.terminal_redirects.reverse();
        assert_eq!(closure_generation(&permuted), expected);
    }

    #[test]
    fn mapping_order_uses_topological_owner_rank_before_source() {
        let mut ranked = fixture();
        ranked.mappings.push(GenerationMapping {
            owner_rank: 0,
            owner_source: "z-parent".into(),
            target: key("acme/standards", "STD-ABCDEFGHJKMN"),
            replacement: key("z-parent", "STD-ABCDEFGHJKMN"),
            rationale: key("z-parent", "DEC-ABCDEFGHJKMN"),
        });
        let expected = closure_generation(&ranked);
        ranked.mappings.reverse();
        assert_eq!(closure_generation(&ranked), expected);

        ranked.mappings[0].owner_rank = 2;
        assert_ne!(closure_generation(&ranked), expected);
    }

    #[test]
    fn every_answer_affecting_section_changes_generation() {
        type GenerationMutation = Box<dyn Fn(&mut GraphGenerationInput)>;

        let original = fixture();
        let expected = closure_generation(&original);

        let mut mutations: Vec<GenerationMutation> = vec![
            Box::new(|input| input.recursive = false),
            Box::new(|input| input.root_source.push('2')),
            Box::new(|input| input.root_corpus_path.push('2')),
            Box::new(|input| input.root_config_bytes.push(b'2')),
            Box::new(|input| input.root_manifest_bytes.as_mut().unwrap().push(b'2')),
            Box::new(|input| input.root_files[0].bytes.push(b'2')),
            Box::new(|input| input.inherited_nodes[0].canonical_digest.push('2')),
            Box::new(|input| input.inherited_nodes[0].config_bytes.push(b'2')),
            Box::new(|input| input.inherited_nodes[0].manifest_bytes = Some(b"nested".to_vec())),
            Box::new(|input| input.inherited_nodes[0].files[0].bytes.push(b'2')),
            Box::new(|input| input.edges[0].alias.push('2')),
            Box::new(|input| input.mappings[0].rationale.canonical_id.push('2')),
            Box::new(|input| input.terminal_redirects[0].terminal.canonical_id.push('2')),
        ];

        for mutate in mutations.drain(..) {
            let mut changed = original.clone();
            mutate(&mut changed);
            assert_ne!(closure_generation(&changed), expected);
        }
    }

    #[test]
    fn manifest_absence_differs_from_empty_manifest() {
        let mut absent = fixture();
        absent.root_manifest_bytes = None;
        let mut empty = fixture();
        empty.root_manifest_bytes = Some(Vec::new());
        assert_ne!(closure_generation(&absent), closure_generation(&empty));
    }
}
