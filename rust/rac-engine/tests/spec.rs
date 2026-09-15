//! Artifact-spec conformance: the Rust structs loaded from the embedded
//! `artifact-specs.json` must match the live oracle registry, field for field
//! and in order. Regenerate the vector with `rust/spec/gen_vectors_spec.py`.

use std::fs;
use std::path::Path;

use rac_engine::spec::{
    available_schemas, relationship_descriptions, snake, spec_for, specs, RELATIONSHIP_SECTIONS,
};
use serde_json::Value;

fn vectors() -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/vectors/spec.json");
    let text = fs::read_to_string(&path).expect("read spec.json");
    serde_json::from_str(&text).expect("parse spec.json")
}

fn str_vec(v: &Value) -> Vec<String> {
    v.as_array()
        .unwrap()
        .iter()
        .map(|x| x.as_str().unwrap().to_string())
        .collect()
}

/// `[[k, [v...]]]` -> ordered `Vec<(String, Vec<String>)>`.
fn pair_list_map(v: &Value) -> Vec<(String, Vec<String>)> {
    v.as_array()
        .unwrap()
        .iter()
        .map(|p| {
            let a = p.as_array().unwrap();
            (a[0].as_str().unwrap().to_string(), str_vec(&a[1]))
        })
        .collect()
}

/// `[[k, v]]` -> ordered `Vec<(String, String)>`.
fn pair_str_map(v: &Value) -> Vec<(String, String)> {
    v.as_array()
        .unwrap()
        .iter()
        .map(|p| {
            let a = p.as_array().unwrap();
            (
                a[0].as_str().unwrap().to_string(),
                a[1].as_str().unwrap().to_string(),
            )
        })
        .collect()
}

#[test]
fn specs_match_oracle_registry() {
    let data = vectors();
    let want_specs = data["artifact_specs"].as_array().unwrap();
    let got = specs();

    assert_eq!(got.len(), want_specs.len(), "spec count");

    // Registry order is load-bearing.
    let want_names = str_vec(&data["names"]);
    assert_eq!(available_schemas(), want_names, "registry order");

    for (g, w) in got.iter().zip(want_specs) {
        let name = w["name"].as_str().unwrap();
        assert_eq!(g.name, name, "name");
        assert_eq!(
            g.display,
            w["display"].as_str().unwrap(),
            "display of {name}"
        );
        assert_eq!(g.required, str_vec(&w["required"]), "required of {name}");
        assert_eq!(
            g.recommended,
            str_vec(&w["recommended"]),
            "recommended of {name}"
        );
        assert_eq!(g.optional, str_vec(&w["optional"]), "optional of {name}");
        assert_eq!(
            g.metadata,
            pair_list_map(&w["metadata"]),
            "metadata of {name}"
        );
        assert_eq!(
            g.retired_status,
            str_vec(&w["retired_status"]),
            "retired_status of {name}"
        );
        assert_eq!(
            g.descriptions,
            pair_str_map(&w["descriptions"]),
            "descriptions of {name}"
        );
        assert_eq!(
            g.guidance,
            pair_list_map(&w["guidance"]),
            "guidance of {name}"
        );
        assert_eq!(
            g.synonyms,
            pair_str_map(&w["synonyms"]),
            "synonyms of {name}"
        );
        assert_eq!(
            g.id_field,
            w["id_field"].as_str().map(str::to_string),
            "id_field of {name}"
        );
        assert_eq!(
            g.starter_bodies,
            pair_str_map(&w["starter_bodies"]),
            "starter_bodies of {name}"
        );
        // `expected` = required + recommended.
        assert_eq!(g.expected(), str_vec(&w["expected"]), "expected of {name}");
    }
}

#[test]
fn spec_for_and_lookup_helpers() {
    assert!(spec_for("unknown").is_none());
    let decision = spec_for("decision").expect("decision spec");
    // Ordered metadata lookup preserves declared order.
    assert_eq!(
        decision.metadata_values("status").unwrap(),
        ["Proposed", "Accepted", "Superseded", "Deprecated"]
    );
    assert_eq!(
        decision.metadata_values("category").unwrap(),
        ["Architecture", "Product", "Process", "Technical", "Other"]
    );
    // Per-spec synonyms.
    assert_eq!(
        decision.synonym("alternatives"),
        Some("alternatives considered")
    );
    let roadmap = spec_for("roadmap").unwrap();
    assert_eq!(roadmap.synonym("success metrics"), Some("success measures"));
    assert_eq!(roadmap.synonym("alternatives"), None);
}

#[test]
fn relationship_sections_are_canonical() {
    let data = vectors();
    let want = str_vec(&data["relationship_sections"]);
    let got: Vec<String> = RELATIONSHIP_SECTIONS
        .iter()
        .map(|(canonical, _)| canonical.to_string())
        .collect();
    assert_eq!(got, want, "RELATIONSHIP_SECTIONS canonical order");

    // Snake keys derive by replacing spaces with underscores.
    for (canonical, snake_key) in RELATIONSHIP_SECTIONS.iter() {
        assert_eq!(&snake(canonical), snake_key, "snake key for {canonical}");
    }
}

#[test]
fn relationship_descriptions_present_and_ordered() {
    // Descriptions round-trip from the embedded JSON; every canonical section
    // that carries a description resolves to a non-empty string.
    let descs = relationship_descriptions();
    assert!(!descs.is_empty(), "relationship_descriptions non-empty");
    for (key, text) in descs {
        assert!(!text.is_empty(), "description for {key} non-empty");
    }
    // A couple of anchors from the contract.
    let map: std::collections::HashMap<_, _> = descs
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();
    assert_eq!(
        map.get("related requirements").copied(),
        Some("Requirement artifacts this artifact references")
    );
    assert!(map.contains_key("verified by"));
    assert!(map.contains_key("applies to"));
}
