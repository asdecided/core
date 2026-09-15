//! Oracle-vector conformance tests for `rac_engine::pycompat`.
//!
//! Vectors: tests/vectors/pycompat.json, generated from the CPython 3.11
//! oracle by rust/spec/gen_vectors_pycompat.py. Floats travel as IEEE-754
//! bit patterns so equality is exact (including -0.0).

use rac_engine::pycompat::*;
use serde_json::Value;

fn vectors() -> Value {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/vectors/pycompat.json");
    let text = std::fs::read_to_string(path).expect("vector file readable");
    serde_json::from_str(&text).expect("vector file parses")
}

fn rows<'a>(v: &'a Value, section: &str) -> &'a Vec<Value> {
    v[section]
        .as_array()
        .unwrap_or_else(|| panic!("section {section} present"))
}

#[test]
fn casefold_matches_oracle() {
    let v = vectors();
    let mut n = 0;
    for row in rows(&v, "casefold") {
        let input = row[0].as_str().unwrap();
        let expected = row[1].as_str().unwrap();
        assert_eq!(py_casefold(input), expected, "casefold({:?})", input);
        n += 1;
    }
    assert!(n >= 500, "expected substantial casefold coverage, got {n}");
}

#[test]
fn strip_matches_oracle() {
    let v = vectors();
    for row in rows(&v, "strip") {
        let input = row[0].as_str().unwrap();
        let exp = row[1].as_array().unwrap();
        assert_eq!(
            py_strip(input),
            exp[0].as_str().unwrap(),
            "strip({input:?})"
        );
        assert_eq!(
            py_lstrip(input),
            exp[1].as_str().unwrap(),
            "lstrip({input:?})"
        );
        assert_eq!(
            py_rstrip(input),
            exp[2].as_str().unwrap(),
            "rstrip({input:?})"
        );
    }
}

#[test]
fn is_space_matches_oracle() {
    let v = vectors();
    for row in rows(&v, "is_space") {
        let cp = row[0].as_u64().unwrap() as u32;
        let expected = row[1].as_bool().unwrap();
        let c = char::from_u32(cp).unwrap();
        assert_eq!(py_is_space(c), expected, "is_space(U+{cp:04X})");
    }
}

#[test]
fn splitlines_matches_oracle() {
    let v = vectors();
    for row in rows(&v, "splitlines") {
        let input = row[0].as_str().unwrap();
        let expected: Vec<&str> = row[1]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p.as_str().unwrap())
            .collect();
        assert_eq!(py_splitlines(input), expected, "splitlines({input:?})");
    }
}

#[test]
fn repr_matches_oracle() {
    let v = vectors();
    for row in rows(&v, "repr") {
        let input = row[0].as_str().unwrap();
        let expected = row[1].as_str().unwrap();
        assert_eq!(py_repr_str(input), expected, "repr({input:?})");
    }
}

#[test]
fn float_repr_matches_oracle() {
    let v = vectors();
    for row in rows(&v, "float_repr") {
        let x = f64::from_bits(row[0].as_u64().unwrap());
        let expected = row[1].as_str().unwrap();
        assert_eq!(py_float_repr(x), expected, "float_repr({x:e})");
    }
}

#[test]
fn round_matches_oracle() {
    let v = vectors();
    for row in rows(&v, "round") {
        let x = f64::from_bits(row[0].as_u64().unwrap());
        let nd = row[1].as_i64().unwrap() as i32;
        let expected_bits = row[2].as_u64().unwrap();
        let got = py_round(x, nd);
        assert_eq!(
            got.to_bits(),
            expected_bits,
            "round({x:e}, {nd}) = {got:e}, oracle {:e}",
            f64::from_bits(expected_bits)
        );
    }
}

#[test]
fn format_1f_matches_oracle() {
    let v = vectors();
    for row in rows(&v, "format_1f") {
        let x = f64::from_bits(row[0].as_u64().unwrap());
        let expected = row[1].as_str().unwrap();
        assert_eq!(py_format_1f(x), expected, "format_1f({x:e})");
    }
}

#[test]
fn percent0_matches_oracle() {
    let v = vectors();
    for row in rows(&v, "percent0") {
        let x = f64::from_bits(row[0].as_u64().unwrap());
        let expected = row[1].as_str().unwrap();
        assert_eq!(py_format_percent0(x), expected, "percent0({x:e})");
    }
}

/// Phase-3 divergence-hunt hook: point PYCOMPAT_FUZZ_VECTORS at a JSON file
/// with the same section shapes as pycompat.json (oracle-generated, any
/// size) and this test replays every section it finds. No-op when unset.
#[test]
fn fuzz_vectors_from_env() {
    let Ok(path) = std::env::var("PYCOMPAT_FUZZ_VECTORS") else {
        return;
    };
    let text = std::fs::read_to_string(&path).expect("fuzz vector file readable");
    let v: Value = serde_json::from_str(&text).expect("fuzz vector file parses");
    let mut n = 0usize;
    if let Some(rows) = v["casefold"].as_array() {
        for row in rows {
            assert_eq!(
                py_casefold(row[0].as_str().unwrap()),
                row[1].as_str().unwrap(),
                "casefold({:?})",
                row[0]
            );
            n += 1;
        }
    }
    if let Some(rows) = v["strip"].as_array() {
        for row in rows {
            let input = row[0].as_str().unwrap();
            let exp = row[1].as_array().unwrap();
            assert_eq!(
                py_strip(input),
                exp[0].as_str().unwrap(),
                "strip({input:?})"
            );
            assert_eq!(
                py_lstrip(input),
                exp[1].as_str().unwrap(),
                "lstrip({input:?})"
            );
            assert_eq!(
                py_rstrip(input),
                exp[2].as_str().unwrap(),
                "rstrip({input:?})"
            );
            n += 1;
        }
    }
    if let Some(rows) = v["splitlines"].as_array() {
        for row in rows {
            let input = row[0].as_str().unwrap();
            let expected: Vec<&str> = row[1]
                .as_array()
                .unwrap()
                .iter()
                .map(|p| p.as_str().unwrap())
                .collect();
            assert_eq!(py_splitlines(input), expected, "splitlines({input:?})");
            n += 1;
        }
    }
    if let Some(rows) = v["repr"].as_array() {
        for row in rows {
            let input = row[0].as_str().unwrap();
            assert_eq!(
                py_repr_str(input),
                row[1].as_str().unwrap(),
                "repr({input:?})"
            );
            n += 1;
        }
    }
    if let Some(rows) = v["float_repr"].as_array() {
        for row in rows {
            let x = f64::from_bits(row[0].as_u64().unwrap());
            assert_eq!(
                py_float_repr(x),
                row[1].as_str().unwrap(),
                "float_repr({x:e})"
            );
            n += 1;
        }
    }
    if let Some(rows) = v["round"].as_array() {
        for row in rows {
            let x = f64::from_bits(row[0].as_u64().unwrap());
            let nd = row[1].as_i64().unwrap() as i32;
            let expected_bits = row[2].as_u64().unwrap();
            let got = py_round(x, nd);
            assert_eq!(
                got.to_bits(),
                expected_bits,
                "round({x:e}, {nd}) = {got:e}, oracle {:e}",
                f64::from_bits(expected_bits)
            );
            n += 1;
        }
    }
    if let Some(rows) = v["format_1f"].as_array() {
        for row in rows {
            let x = f64::from_bits(row[0].as_u64().unwrap());
            assert_eq!(
                py_format_1f(x),
                row[1].as_str().unwrap(),
                "format_1f({x:e})"
            );
            n += 1;
        }
    }
    if let Some(rows) = v["percent0"].as_array() {
        for row in rows {
            let x = f64::from_bits(row[0].as_u64().unwrap());
            assert_eq!(
                py_format_percent0(x),
                row[1].as_str().unwrap(),
                "percent0({x:e})"
            );
            n += 1;
        }
    }
    eprintln!("fuzz_vectors_from_env: {n} rows replayed from {path}");
}

#[test]
fn re_classes_match_oracle() {
    let v = vectors();
    for row in rows(&v, "re_digit") {
        let cp = row[0].as_u64().unwrap() as u32;
        let expected = row[1].as_bool().unwrap();
        assert_eq!(
            is_re_digit(char::from_u32(cp).unwrap()),
            expected,
            "re \\d U+{cp:04X}"
        );
    }
    for row in rows(&v, "re_word") {
        let cp = row[0].as_u64().unwrap() as u32;
        let expected = row[1].as_bool().unwrap();
        assert_eq!(
            is_re_word(char::from_u32(cp).unwrap()),
            expected,
            "re \\w U+{cp:04X}"
        );
    }
}
