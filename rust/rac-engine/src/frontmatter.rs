//! Frontmatter parsing — port of `src/rac/core/frontmatter.py` plus the
//! bounded PyYAML-1.1 SafeLoader subset it rides on (PORT-CONTRACT.d/02).
//!
//! This is parity landmine #1: the oracle is PyYAML 6.0.3's pure-Python
//! `SafeLoader` (full YAML 1.1) subclassed with three guards — duplicate-key
//! rejection, alias rejection, and a 32-level node-count depth cap. Byte
//! parity requires reproducing PyYAML's implicit resolution, its error
//! *problem* strings, and CPython `repr()` formatting inside issue messages.
//! The scanner/parser/composer/constructor below are direct ports of the
//! corresponding PyYAML modules (message strings verbatim).
//!
//! Known oracle crashes (PORT-CONTRACT decision 3): several inputs crash the
//! oracle with uncaught non-YAML exceptions (unhashable mapping keys,
//! explicit-tag/value mismatches like `!!int ''`, out-of-range dates such as
//! `2026-13-01`, `!!map` on a non-empty scalar/sequence, and CPython's
//! 4300-digit int<->str conversion limit). This port does NOT
//! crash: every such path returns a distinguishable internal issue (code
//! `internal-oracle-divergence`) whose message mirrors the Python exception
//! (`"TypeError: unhashable type: 'list'"`, ...). The marker is intentional
//! and the parity harness treats it as the documented divergence class.
//!
//! Integers are unbounded like Python's `int`: values
//! beyond i64 construct `Yaml::BigInt` (sign + decimal digits) instead of
//! overflowing, and duplicate-key equality / `repr()` / validator messages
//! follow CPython semantics for them exactly.

use std::collections::HashMap;

use crate::pycompat::{py_float_repr, py_repr_str, py_strip};

// ---------------------------------------------------------------------------
// Limits (src/rac/core/limits.py)
// ---------------------------------------------------------------------------

pub const DEFAULT_MAX_FILE_BYTES: u64 = 1 << 20; // 1 MiB
pub const MAX_FRONTMATTER_BYTES: usize = 64 << 10; // 64 KiB
pub const MAX_FRONTMATTER_DEPTH: usize = 32;
pub const SUPPORTED_SCHEMA_VERSIONS: &[i64] = &[1];

const SUPPORTED_FIELDS: [&str; 5] = ["schema_version", "id", "type", "relationships", "tags"];

/// `exceeds_byte_cap(text, cap)`: true when `text` exceeds `cap` UTF-8 bytes.
/// (The oracle's char-count shortcuts are a pure optimization; Rust `len()`
/// is already the UTF-8 byte length.)
pub fn exceeds_byte_cap(text: &str, cap: usize) -> bool {
    text.len() > cap
}

/// The per-file byte cap at the READ stage.
///
/// The oracle's `parse_file` runs `fh.read(cap + 1)`, which CRASHES the
/// oracle uncaught for huge caps (PORT-CONTRACT decision-3 marker class),
/// with the boundary verified empirically against CPython 3.11:
///   - cap >= 2^63 - 1  (`cap + 1 > sys.maxsize`):
///     `OverflowError: cannot fit 'int' into an index-sized integer`
///   - 2^63 - 34 <= cap <= 2^63 - 2  (`cap + 1` exceeds the bytes-object
///     size limit `PY_SSIZE_T_MAX - 33`):
///     `OverflowError: byte string is too large`
///   - below that, down to roughly the machine's allocatable memory, the
///     oracle raises `MemoryError` — an ENVIRONMENT-DEPENDENT crash that
///     cannot be mirrored deterministically and is deliberately NOT
///     mirrored (the Rust engine reads incrementally and never
///     preallocates the cap).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileCap {
    Cap(u64),
    /// Every file READ crashes the oracle with this exception line.
    OracleCrash(&'static str),
}

/// `cap + 1 > sys.maxsize`, i.e. cap >= 2^63 - 1.
const ORACLE_READ_OVERFLOW_MIN: i128 = i64::MAX as i128;
/// `cap + 1` over the CPython bytes allocation limit (PY_SSIZE_T_MAX - 33).
const ORACLE_READ_TOOLARGE_MIN: i128 = i64::MAX as i128 - 33;

/// The per-file byte cap, honoring `DECIDED_MAX_FILE_BYTES` — Python `int()`
/// semantics (Unicode digits, underscores, unbounded magnitude; unparseable
/// or non-positive overrides fall back to the default). Shared parser with
/// `markdown::max_file_bytes_from` so the read and parse stages agree.
pub fn file_cap() -> FileCap {
    match std::env::var("DECIDED_MAX_FILE_BYTES") {
        Ok(raw) => file_cap_from(Some(&raw)),
        Err(_) => FileCap::Cap(DEFAULT_MAX_FILE_BYTES),
    }
}

pub fn file_cap_from(raw: Option<&str>) -> FileCap {
    if let Some(raw) = raw {
        if let Some(v) = crate::markdown::py_parse_int(raw) {
            if v >= ORACLE_READ_OVERFLOW_MIN {
                return FileCap::OracleCrash(
                    "OverflowError: cannot fit 'int' into an index-sized integer",
                );
            }
            if v >= ORACLE_READ_TOOLARGE_MIN {
                return FileCap::OracleCrash("OverflowError: byte string is too large");
            }
            if v > 0 {
                return FileCap::Cap(v as u64);
            }
        }
    }
    FileCap::Cap(DEFAULT_MAX_FILE_BYTES)
}

// ---------------------------------------------------------------------------
// Issue / metadata models (src/rac/core/models.py, metadata.py)
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
pub struct Issue {
    pub severity: &'static str,
    pub code: String,
    pub message: String,
    pub line: Option<i64>,
}

impl Issue {
    fn error(code: &str, message: String) -> Issue {
        Issue {
            severity: "error",
            code: code.to_string(),
            message,
            line: None,
        }
    }
}

/// `ArtifactMetadata.schema_version`: Python keeps the parsed int as-is,
/// which can exceed i64 (an unsupported-but-integer version is stored with
/// only an issue recorded). `Display` matches Python `str(int)`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SchemaVersion {
    Int(i64),
    Big(BigInt),
}

impl std::fmt::Display for SchemaVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SchemaVersion::Int(v) => write!(f, "{v}"),
            SchemaVersion::Big(b) => write!(f, "{b}"),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ArtifactMetadata {
    pub schema_version: SchemaVersion,
    pub id: Option<String>,
    pub artifact_type: Option<String>,
    /// Ordered (kind -> targets), preserving YAML document order.
    pub relationships: Vec<(String, Vec<String>)>,
    pub tags: Vec<String>,
    pub provenance: &'static str,
}

/// Canonical (uppercase) form of an artifact ID (Python `strip().upper()`).
pub fn normalize_id(value: &str) -> String {
    py_strip(value).to_uppercase()
}

fn is_crockford(c: char) -> bool {
    matches!(c, '0'..='9' | 'A'..='H' | 'J' | 'K' | 'M' | 'N' | 'P'..='T' | 'V'..='Z')
}

/// `^[A-Z][A-Z0-9]{1,9}-[0-9A-HJKMNP-TV-Z]{12}$` over the normalized id.
pub fn is_valid_id(value: &str) -> bool {
    let n = normalize_id(value);
    let chars: Vec<char> = n.chars().collect();
    if chars.is_empty() || !chars[0].is_ascii_uppercase() {
        return false;
    }
    // Backtrack over the {1,9} key tail exactly as the regex engine would.
    for keylen in 1..=9usize {
        let dash = 1 + keylen;
        if chars.len() != dash + 13 {
            continue;
        }
        if !chars[1..dash]
            .iter()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
        {
            continue;
        }
        if chars[dash] != '-' {
            continue;
        }
        if chars[dash + 1..].iter().all(|c| is_crockford(*c)) {
            return true;
        }
    }
    false
}

// ---------------------------------------------------------------------------
// Value model — what PyYAML SafeLoader construction can yield here
// ---------------------------------------------------------------------------

/// Arbitrary-precision integer (sign + decimal digits), mirroring Python's
/// unbounded `int` for values outside i64 (PORT-CONTRACT 02 §4: the YAML 1.1
/// int constructor never overflows).
///
/// Invariant: the magnitude never fits i64 (smaller values construct
/// `Yaml::Int`), and `digits` has no leading zeros, so cross-class equality
/// with `Int`/`Bool` is always false and `BigInt` equality is digit equality.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BigInt {
    pub neg: bool,
    /// Decimal magnitude digits, most significant first.
    pub digits: String,
}

impl std::fmt::Display for BigInt {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.neg {
            f.write_str("-")?;
        }
        f.write_str(&self.digits)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Yaml {
    Null,
    Bool(bool),
    Int(i64),
    /// Python bignum — an int whose value is outside the i64 range.
    BigInt(BigInt),
    Float(f64),
    Str(String),
    Date {
        year: i64,
        month: u32,
        day: u32,
    },
    DateTime {
        year: i64,
        month: u32,
        day: u32,
        hour: u32,
        minute: u32,
        second: u32,
        micro: u32,
        /// UTC offset in seconds; None = naive.
        tz: Option<i64>,
    },
    Bytes(Vec<u8>),
    List(Vec<Yaml>),
    /// Python tuples (from `!!omap` / `!!pairs` entries).
    Tuple(Vec<Yaml>),
    /// Insertion-ordered mapping (Python dict preserves document order).
    Map(Vec<(Yaml, Yaml)>),
    /// From `!!set`; stored in first-occurrence order.
    Set(Vec<Yaml>),
}

/// Python `==` over the constructed values: numeric cross-class equality
/// (`1 == True == 1.0`), and NaN keys compare equal to each other because
/// PyYAML returns the one shared `nan_value` object (identity hit in the
/// oracle's duplicate-key `set`).
pub fn py_eq(a: &Yaml, b: &Yaml) -> bool {
    fn num(v: &Yaml) -> Option<f64> {
        match v {
            Yaml::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
            Yaml::Int(i) => Some(*i as f64),
            Yaml::Float(f) => Some(*f),
            _ => None,
        }
    }
    match (a, b) {
        (Yaml::Null, Yaml::Null) => true,
        (Yaml::Str(x), Yaml::Str(y)) => x == y,
        (Yaml::Bytes(x), Yaml::Bytes(y)) => x == y,
        (
            Yaml::Date { year, month, day },
            Yaml::Date {
                year: y2,
                month: m2,
                day: d2,
            },
        ) => year == y2 && month == m2 && day == d2,
        (Yaml::DateTime { .. }, Yaml::DateTime { .. }) => datetime_eq(a, b),
        (Yaml::Tuple(x), Yaml::Tuple(y)) => {
            x.len() == y.len() && x.iter().zip(y).all(|(p, q)| py_eq(p, q))
        }
        (Yaml::List(x), Yaml::List(y)) => {
            x.len() == y.len() && x.iter().zip(y).all(|(p, q)| py_eq(p, q))
        }
        (Yaml::BigInt(x), Yaml::BigInt(y)) => x == y,
        // Invariant: a BigInt never fits i64, so it can never equal an
        // Int or Bool (Python compares the mathematical values).
        (Yaml::BigInt(_), Yaml::Int(_) | Yaml::Bool(_))
        | (Yaml::Int(_) | Yaml::Bool(_), Yaml::BigInt(_)) => false,
        // Python bignum == float compares mathematically (exactly).
        (Yaml::BigInt(x), Yaml::Float(f)) | (Yaml::Float(f), Yaml::BigInt(x)) => {
            bigint_eq_float(x, *f)
        }
        (Yaml::BigInt(_), _) | (_, Yaml::BigInt(_)) => false,
        _ => match (num(a), num(b)) {
            (Some(x), Some(y)) => {
                if x.is_nan() && y.is_nan() {
                    // PyYAML nan_value identity (verified: `.nan:` twice is a dup).
                    matches!((a, b), (Yaml::Float(_), Yaml::Float(_)))
                } else {
                    exact_num_eq(a, b, x, y)
                }
            }
            _ => false,
        },
    }
}

/// Exact Python numeric equality across bool/int/float without f64 precision
/// loss on large i64s.
fn exact_num_eq(a: &Yaml, b: &Yaml, x: f64, y: f64) -> bool {
    fn as_int(v: &Yaml) -> Option<i64> {
        match v {
            Yaml::Bool(b) => Some(*b as i64),
            Yaml::Int(i) => Some(*i),
            _ => None,
        }
    }
    match (as_int(a), as_int(b)) {
        (Some(i), Some(j)) => i == j,
        (Some(i), None) => float_eq_int(y, i),
        (None, Some(j)) => float_eq_int(x, j),
        (None, None) => x == y,
    }
}

fn float_eq_int(f: f64, i: i64) -> bool {
    if !f.is_finite() || f != f.trunc() {
        return false;
    }
    if !(-9.223_372_036_854_776E18..9.223_372_036_854_776E18).contains(&f) {
        return false;
    }
    (f as i64) == i && (f as i64) as f64 == f
}

// ---------------------------------------------------------------------------
// Arbitrary-precision decimal magnitude — just enough bignum for PyYAML int
// construction and Python's exact cross-class numeric equality.
// ---------------------------------------------------------------------------

/// CPython's default `sys.get_int_max_str_digits()` (Python 3.11+): int<->str
/// conversions beyond this many decimal digits raise `ValueError` — uncaught
/// by the oracle, so decision-3 internal markers mirror them.
const INT_MAX_STR_DIGITS: usize = 4300;

/// `str(int)` / `repr(int)` over the limit (message has no digit count).
fn int_to_str_limit_err() -> YErr {
    YErr::Internal(
        "ValueError: Exceeds the limit (4300 digits) for integer string conversion; \
         use sys.set_int_max_str_digits() to increase the limit"
            .to_string(),
    )
}

/// `int(str)` (base 10 only) over the limit (message carries the count).
fn int_parse_limit_err(ndigits: usize) -> YErr {
    YErr::Internal(format!(
        "ValueError: Exceeds the limit (4300 digits) for integer string conversion: \
         value has {ndigits} digits; use sys.set_int_max_str_digits() to increase the limit"
    ))
}

/// Unsigned magnitude in base 1e9 limbs, least significant first. Base 1e9
/// keeps the arithmetic O(digits^2/9) worst case and makes the decimal
/// rendering a straight concatenation.
#[derive(Clone, Debug)]
struct Mag(Vec<u32>);

const MAG_BASE: u64 = 1_000_000_000;

impl Mag {
    fn zero() -> Mag {
        Mag(vec![0])
    }

    fn from_u64(mut v: u64) -> Mag {
        let mut limbs = Vec::new();
        loop {
            limbs.push((v % MAG_BASE) as u32);
            v /= MAG_BASE;
            if v == 0 {
                break;
            }
        }
        Mag(limbs)
    }

    fn is_zero(&self) -> bool {
        self.0.iter().all(|&l| l == 0)
    }

    fn trim(&mut self) {
        while self.0.len() > 1 && *self.0.last().unwrap() == 0 {
            self.0.pop();
        }
    }

    /// self = self * m + a  (m, a < 2^32).
    fn mul_add_small(&mut self, m: u64, a: u64) {
        let mut carry = a;
        for limb in &mut self.0 {
            let v = *limb as u64 * m + carry;
            *limb = (v % MAG_BASE) as u32;
            carry = v / MAG_BASE;
        }
        while carry > 0 {
            self.0.push((carry % MAG_BASE) as u32);
            carry /= MAG_BASE;
        }
        self.trim();
    }

    fn add(&mut self, other: &Mag) {
        if other.0.len() > self.0.len() {
            self.0.resize(other.0.len(), 0);
        }
        let mut carry = 0u64;
        for (i, limb) in self.0.iter_mut().enumerate() {
            let v = *limb as u64 + other.0.get(i).map_or(0, |&l| l as u64) + carry;
            *limb = (v % MAG_BASE) as u32;
            carry = v / MAG_BASE;
        }
        if carry > 0 {
            self.0.push(carry as u32);
        }
    }

    /// self - other; requires self >= other.
    fn sub(&mut self, other: &Mag) {
        let mut borrow = 0i64;
        for (i, limb) in self.0.iter_mut().enumerate() {
            let mut v = *limb as i64 - other.0.get(i).map_or(0, |&l| l as i64) - borrow;
            if v < 0 {
                v += MAG_BASE as i64;
                borrow = 1;
            } else {
                borrow = 0;
            }
            *limb = v as u32;
        }
        debug_assert_eq!(borrow, 0, "Mag::sub underflow");
        self.trim();
    }

    fn cmp_mag(&self, other: &Mag) -> std::cmp::Ordering {
        let (mut a, mut b) = (self.clone(), other.clone());
        a.trim();
        b.trim();
        if a.0.len() != b.0.len() {
            return a.0.len().cmp(&b.0.len());
        }
        for (x, y) in a.0.iter().rev().zip(b.0.iter().rev()) {
            if x != y {
                return x.cmp(y);
            }
        }
        std::cmp::Ordering::Equal
    }

    /// Decimal digits, most significant first, no leading zeros.
    fn to_decimal(&self) -> String {
        let mut limbs = self.0.clone();
        while limbs.len() > 1 && *limbs.last().unwrap() == 0 {
            limbs.pop();
        }
        let mut out = format!("{}", limbs.last().unwrap());
        for limb in limbs.iter().rev().skip(1) {
            out.push_str(&format!("{limb:09}"));
        }
        out
    }

    /// The signed value when it fits i64 (magnitude up to 2^63 when `neg`).
    fn to_i64(&self, neg: bool) -> Option<i64> {
        let mut acc: i64 = 0;
        for &limb in self.0.iter().rev() {
            acc = acc.checked_mul(MAG_BASE as i64)?;
            acc = if neg {
                acc.checked_sub(limb as i64)?
            } else {
                acc.checked_add(limb as i64)?
            };
        }
        Some(acc)
    }
}

/// The `Yaml` value for a signed magnitude: `Int` when it fits i64, else the
/// exact Python bignum.
fn yaml_int(neg: bool, mag: &Mag) -> Yaml {
    match mag.to_i64(neg) {
        Some(v) => Yaml::Int(v),
        None => Yaml::BigInt(BigInt {
            neg,
            digits: mag.to_decimal(),
        }),
    }
}

/// Python `bignum == float` (exact): true iff the float is finite, integral,
/// and its exact value has the same sign and decimal digits.
fn bigint_eq_float(x: &BigInt, f: f64) -> bool {
    if !f.is_finite() || f != f.trunc() || f == 0.0 {
        return false;
    }
    if (f < 0.0) != x.neg {
        return false;
    }
    f64_integral_magnitude(f.abs()) == x.digits
}

/// Exact decimal digits of a positive integral f64 (mantissa * 2^exp).
fn f64_integral_magnitude(f: f64) -> String {
    let bits = f.to_bits();
    let exp = ((bits >> 52) & 0x7ff) as i64;
    let frac = bits & ((1u64 << 52) - 1);
    let (mut m, mut e) = if exp == 0 {
        (frac, -1074i64)
    } else {
        (frac | (1 << 52), exp - 1075)
    };
    // Integral input: any negative exponent shifts out exactly.
    while e < 0 {
        m >>= 1;
        e += 1;
    }
    let mut mag = Mag::from_u64(m);
    for _ in 0..e {
        mag.mul_add_small(2, 0);
    }
    mag.to_decimal()
}

fn datetime_eq(a: &Yaml, b: &Yaml) -> bool {
    if let (
        Yaml::DateTime {
            year,
            month,
            day,
            hour,
            minute,
            second,
            micro,
            tz,
        },
        Yaml::DateTime {
            year: y2,
            month: m2,
            day: d2,
            hour: h2,
            minute: mi2,
            second: s2,
            micro: us2,
            tz: tz2,
        },
    ) = (a, b)
    {
        match (tz, tz2) {
            (None, None) => {
                (year, month, day, hour, minute, second, micro)
                    == (y2, m2, d2, h2, mi2, s2, us2)
            }
            (Some(o1), Some(o2)) => {
                let u1 = utc_micros(*year, *month, *day, *hour, *minute, *second, *micro) - o1 * 1_000_000;
                let u2 = utc_micros(*y2, *m2, *d2, *h2, *mi2, *s2, *us2) - o2 * 1_000_000;
                u1 == u2
            }
            // Python 3: mixed naive/aware equality is False.
            _ => false,
        }
    } else {
        false
    }
}

fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    // Howard Hinnant's algorithm; proleptic Gregorian.
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn utc_micros(y: i64, mo: u32, d: u32, h: u32, mi: u32, s: u32, us: u32) -> i64 {
    let days = days_from_civil(y, mo as i64, d as i64);
    ((days * 86_400) + h as i64 * 3600 + mi as i64 * 60 + s as i64) * 1_000_000 + us as i64
}

/// If hashing `v` would raise `TypeError` in CPython, the offending type
/// name (`list` / `dict` / `set`), recursing through tuples.
fn unhashable_type_name(v: &Yaml) -> Option<&'static str> {
    match v {
        Yaml::List(_) => Some("list"),
        Yaml::Map(_) => Some("dict"),
        Yaml::Set(_) => Some("set"),
        Yaml::Tuple(items) => items.iter().find_map(unhashable_type_name),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Python repr() of constructed values (drives `{key!r}` in issue messages)
// ---------------------------------------------------------------------------

fn py_repr_bytes(b: &[u8]) -> String {
    let has_sq = b.contains(&b'\'');
    let has_dq = b.contains(&b'"');
    let quote = if has_sq && !has_dq { b'"' } else { b'\'' };
    let mut out = String::from("b");
    out.push(quote as char);
    for &c in b {
        if c == quote || c == b'\\' {
            out.push('\\');
            out.push(c as char);
        } else if c == b'\t' {
            out.push_str("\\t");
        } else if c == b'\n' {
            out.push_str("\\n");
        } else if c == b'\r' {
            out.push_str("\\r");
        } else if (0x20..0x7f).contains(&c) {
            out.push(c as char);
        } else {
            out.push_str(&format!("\\x{c:02x}"));
        }
    }
    out.push(quote as char);
    out
}

fn py_repr_timedelta(offset_seconds: i64) -> String {
    // datetime.timedelta normalizes to 0 <= seconds < 86400.
    let days = offset_seconds.div_euclid(86_400);
    let secs = offset_seconds.rem_euclid(86_400);
    let mut parts: Vec<String> = Vec::new();
    if days != 0 {
        parts.push(format!("days={days}"));
    }
    if secs != 0 {
        parts.push(format!("seconds={secs}"));
    }
    if parts.is_empty() {
        "datetime.timedelta(0)".to_string()
    } else {
        format!("datetime.timedelta({})", parts.join(", "))
    }
}

fn py_repr_tzinfo(offset_seconds: i64) -> String {
    if offset_seconds == 0 {
        // timezone(timedelta(0)) is the utc singleton (verified).
        "datetime.timezone.utc".to_string()
    } else {
        format!("datetime.timezone({})", py_repr_timedelta(offset_seconds))
    }
}

/// CPython `repr()` for every value the loader can produce. Fallible: a
/// bignum beyond the 4300-digit conversion limit raises `ValueError` in the
/// oracle (uncaught — decision-3 internal marker), at any nesting depth.
fn py_repr(v: &Yaml) -> Result<String, YErr> {
    fn join(items: &[Yaml]) -> Result<String, YErr> {
        Ok(items
            .iter()
            .map(py_repr)
            .collect::<Result<Vec<_>, _>>()?
            .join(", "))
    }
    Ok(match v {
        Yaml::Null => "None".to_string(),
        Yaml::Bool(true) => "True".to_string(),
        Yaml::Bool(false) => "False".to_string(),
        Yaml::Int(i) => i.to_string(),
        Yaml::BigInt(b) => {
            if b.digits.len() > INT_MAX_STR_DIGITS {
                return Err(int_to_str_limit_err());
            }
            b.to_string()
        }
        Yaml::Float(f) => py_float_repr(*f),
        Yaml::Str(s) => py_repr_str(s),
        Yaml::Date { year, month, day } => {
            format!("datetime.date({year}, {month}, {day})")
        }
        Yaml::DateTime {
            year,
            month,
            day,
            hour,
            minute,
            second,
            micro,
            tz,
        } => {
            let mut out = format!("datetime.datetime({year}, {month}, {day}, {hour}, {minute}");
            if *second != 0 || *micro != 0 {
                out.push_str(&format!(", {second}"));
            }
            if *micro != 0 {
                out.push_str(&format!(", {micro}"));
            }
            if let Some(off) = tz {
                out.push_str(&format!(", tzinfo={}", py_repr_tzinfo(*off)));
            }
            out.push(')');
            out
        }
        Yaml::Bytes(b) => py_repr_bytes(b),
        Yaml::Tuple(items) => match items.len() {
            0 => "()".to_string(),
            1 => format!("({},)", py_repr(&items[0])?),
            _ => format!("({})", join(items)?),
        },
        Yaml::List(items) => format!("[{}]", join(items)?),
        Yaml::Map(pairs) => format!(
            "{{{}}}",
            pairs
                .iter()
                .map(|(k, v)| Ok(format!("{}: {}", py_repr(k)?, py_repr(v)?)))
                .collect::<Result<Vec<_>, YErr>>()?
                .join(", ")
        ),
        Yaml::Set(items) => {
            if items.is_empty() {
                "set()".to_string()
            } else {
                format!("{{{}}}", join(items)?)
            }
        }
    })
}

// ---------------------------------------------------------------------------
// split_frontmatter
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
pub struct FrontmatterSplit {
    pub raw: Option<String>,
    pub body: String,
    pub line_offset: usize,
    pub unterminated: bool,
}

/// Split a leading `---` frontmatter block from `text` — LF-only line split
/// (CRLF leaves `\r` in `raw`), Python-whitespace `.strip()` on delimiter
/// lines (BOM and U+200B are NOT whitespace and defeat the delimiter).
pub fn split_frontmatter(text: &str) -> FrontmatterSplit {
    let lines: Vec<&str> = text.split('\n').collect();
    if py_strip(lines[0]) != "---" {
        return FrontmatterSplit {
            raw: None,
            body: text.to_string(),
            line_offset: 0,
            unterminated: false,
        };
    }
    for i in 1..lines.len() {
        let stripped = py_strip(lines[i]);
        if stripped == "---" || stripped == "..." {
            return FrontmatterSplit {
                raw: Some(lines[1..i].join("\n")),
                body: lines[i + 1..].join("\n"),
                line_offset: i + 1,
                unterminated: false,
            };
        }
    }
    FrontmatterSplit {
        raw: None,
        body: text.to_string(),
        line_offset: 0,
        unterminated: true,
    }
}

// ---------------------------------------------------------------------------
// YAML engine error type
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
enum YErr {
    /// MarkedYAMLError — only the `problem` string reaches output.
    Marked(String),
    /// yaml.reader.ReaderError — the full multi-line `str(exc)`.
    Reader(String),
    /// The oracle would crash with an uncaught non-YAML exception here
    /// (PORT-CONTRACT decision 3). Message mirrors `f"{type}: {exc}"`.
    Internal(String),
}

// ---------------------------------------------------------------------------
// YAML 1.1 implicit resolver (yaml/resolver.py, regexes matched by hand —
// no regex crate in the workspace)
// ---------------------------------------------------------------------------

const TAG_STR: &str = "tag:yaml.org,2002:str";
const TAG_SEQ: &str = "tag:yaml.org,2002:seq";
const TAG_MAP: &str = "tag:yaml.org,2002:map";
const TAG_BOOL: &str = "tag:yaml.org,2002:bool";
const TAG_INT: &str = "tag:yaml.org,2002:int";
const TAG_FLOAT: &str = "tag:yaml.org,2002:float";
const TAG_MERGE: &str = "tag:yaml.org,2002:merge";
const TAG_NULL: &str = "tag:yaml.org,2002:null";
const TAG_TIMESTAMP: &str = "tag:yaml.org,2002:timestamp";
const TAG_VALUE: &str = "tag:yaml.org,2002:value";

fn match_bool(v: &str) -> bool {
    matches!(
        v,
        "yes" | "Yes" | "YES" | "no" | "No" | "NO" | "true" | "True" | "TRUE" | "false"
            | "False" | "FALSE" | "on" | "On" | "ON" | "off" | "Off" | "OFF"
    )
}

fn eat_sign(b: &[u8]) -> &[u8] {
    if !b.is_empty() && (b[0] == b'-' || b[0] == b'+') {
        &b[1..]
    } else {
        b
    }
}

fn all_in(b: &[u8], pred: impl Fn(u8) -> bool) -> bool {
    b.iter().all(|&c| pred(c))
}

/// `(?:[eE][-+][0-9]+)?` — returns rest after consuming an exponent (which
/// must have a sign), or None if a malformed exponent-like tail is present.
fn strip_signed_exponent(b: &[u8]) -> Option<&[u8]> {
    if b.is_empty() {
        return Some(b);
    }
    if b[0] == b'e' || b[0] == b'E' {
        if b.len() >= 3 && (b[1] == b'-' || b[1] == b'+') && b[2..].iter().all(u8::is_ascii_digit)
        {
            Some(&b[..0])
        } else {
            None
        }
    } else {
        Some(b)
    }
}

fn match_float(v: &str) -> bool {
    let b = v.as_bytes();
    if !b.is_ascii() {
        return false;
    }
    // [-+]?\.(?:inf|Inf|INF)
    {
        let t = eat_sign(b);
        if t == b".inf" || t == b".Inf" || t == b".INF" {
            return true;
        }
    }
    // \.(?:nan|NaN|NAN)  (no sign)
    if b == b".nan" || b == b".NaN" || b == b".NAN" {
        return true;
    }
    // \.[0-9][0-9_]*(?:[eE][-+][0-9]+)?  (no sign)
    if b.first() == Some(&b'.') {
        let t = &b[1..];
        if !t.is_empty() && t[0].is_ascii_digit() {
            let end = t
                .iter()
                .position(|&c| !(c.is_ascii_digit() || c == b'_'))
                .unwrap_or(t.len());
            if let Some(rest) = strip_signed_exponent(&t[end..]) {
                if rest.is_empty() {
                    return true;
                }
            }
        }
    }
    let t = eat_sign(b);
    if t.is_empty() || !t[0].is_ascii_digit() {
        return false;
    }
    // [-+]?[0-9][0-9_]*\.[0-9_]*(?:[eE][-+][0-9]+)?
    // [-+]?[0-9][0-9_]*(?::[0-5]?[0-9])+\.[0-9_]*
    let int_end = t
        .iter()
        .position(|&c| !(c.is_ascii_digit() || c == b'_'))
        .unwrap_or(t.len());
    let rest = &t[int_end..];
    if rest.first() == Some(&b'.') {
        let frac = &rest[1..];
        let frac_end = frac
            .iter()
            .position(|&c| !(c.is_ascii_digit() || c == b'_'))
            .unwrap_or(frac.len());
        if let Some(after) = strip_signed_exponent(&frac[frac_end..]) {
            return after.is_empty();
        }
        return false;
    }
    if rest.first() == Some(&b':') {
        // (?::[0-5]?[0-9])+\.[0-9_]*  — sexagesimal float, no exponent.
        let mut r = rest;
        loop {
            if r.first() != Some(&b':') {
                break;
            }
            r = &r[1..];
            let mut ndig = 0;
            if r.first().is_some_and(|c| (b'0'..=b'5').contains(c))
                && r.get(1).is_some_and(u8::is_ascii_digit)
            {
                ndig = 2;
            } else if r.first().is_some_and(u8::is_ascii_digit) {
                ndig = 1;
            }
            if ndig == 0 {
                return false;
            }
            r = &r[ndig..];
        }
        if r.first() != Some(&b'.') {
            return false;
        }
        return all_in(&r[1..], |c| c.is_ascii_digit() || c == b'_');
    }
    false
}

fn match_int(v: &str) -> bool {
    let b = v.as_bytes();
    if !b.is_ascii() {
        return false;
    }
    let t = eat_sign(b);
    if t.is_empty() {
        return false;
    }
    // 0b[0-1_]+
    if t.len() > 2 && t.starts_with(b"0b") && all_in(&t[2..], |c| c == b'0' || c == b'1' || c == b'_') {
        return true;
    }
    // 0x[0-9a-fA-F_]+
    if t.len() > 2 && t.starts_with(b"0x") && all_in(&t[2..], |c| c.is_ascii_hexdigit() || c == b'_')
    {
        return true;
    }
    // 0[0-7_]+
    if t.len() > 1 && t[0] == b'0' && all_in(&t[1..], |c| (b'0'..=b'7').contains(&c) || c == b'_') {
        return true;
    }
    // 0 | [1-9][0-9_]*
    if t == b"0" {
        return true;
    }
    if t[0].is_ascii_digit() && t[0] != b'0' {
        let end = t
            .iter()
            .position(|&c| !(c.is_ascii_digit() || c == b'_'))
            .unwrap_or(t.len());
        if end == t.len() {
            return true;
        }
        // [1-9][0-9_]*(?::[0-5]?[0-9])+  — sexagesimal int
        let mut r = &t[end..];
        if r.first() != Some(&b':') {
            return false;
        }
        while r.first() == Some(&b':') {
            r = &r[1..];
            let ndig = if r.first().is_some_and(|c| (b'0'..=b'5').contains(c))
                && r.get(1).is_some_and(u8::is_ascii_digit)
            {
                2
            } else if r.first().is_some_and(u8::is_ascii_digit) {
                1
            } else {
                return false;
            };
            r = &r[ndig..];
        }
        return r.is_empty();
    }
    false
}

fn match_null(v: &str) -> bool {
    matches!(v, "~" | "null" | "Null" | "NULL" | "")
}

/// The timestamp *resolver* regex (stricter than the constructor's).
fn match_timestamp(v: &str) -> bool {
    let b = v.as_bytes();
    if !b.is_ascii() {
        return false;
    }
    let d = |i: usize| b.get(i).is_some_and(u8::is_ascii_digit);
    // [0-9]{4}-[0-9]{2}-[0-9]{2}  (date-only, fixed width)
    if b.len() == 10
        && d(0)
        && d(1)
        && d(2)
        && d(3)
        && b[4] == b'-'
        && d(5)
        && d(6)
        && b[7] == b'-'
        && d(8)
        && d(9)
    {
        return true;
    }
    // Full form: [0-9]{4}-[0-9]{1,2}-[0-9]{1,2}([Tt]|[ \t]+)[0-9]{1,2}:[0-9]{2}:[0-9]{2}(\.[0-9]*)?([ \t]*(Z|[-+][0-9]{1,2}(:[0-9]{2})?))?
    let mut i = 0;
    if !(d(0) && d(1) && d(2) && d(3)) {
        return false;
    }
    i += 4;
    if b.get(i) != Some(&b'-') {
        return false;
    }
    i += 1;
    if !d(i) {
        return false;
    }
    i += 1;
    if d(i) {
        i += 1;
    }
    if b.get(i) != Some(&b'-') {
        return false;
    }
    i += 1;
    if !d(i) {
        return false;
    }
    i += 1;
    if d(i) {
        i += 1;
    }
    // separator
    match b.get(i) {
        Some(b'T') | Some(b't') => i += 1,
        Some(b' ') | Some(b'\t') => {
            while matches!(b.get(i), Some(b' ') | Some(b'\t')) {
                i += 1;
            }
        }
        _ => return false,
    }
    if !d(i) {
        return false;
    }
    i += 1;
    if d(i) {
        i += 1;
    }
    if b.get(i) != Some(&b':') || !(d(i + 1) && d(i + 2)) {
        return false;
    }
    i += 3;
    if b.get(i) != Some(&b':') || !(d(i + 1) && d(i + 2)) {
        return false;
    }
    i += 3;
    if b.get(i) == Some(&b'.') {
        i += 1;
        while d(i) {
            i += 1;
        }
    }
    if i == b.len() {
        return true;
    }
    while matches!(b.get(i), Some(b' ') | Some(b'\t')) {
        i += 1;
    }
    match b.get(i) {
        Some(b'Z') => i += 1,
        Some(b'-') | Some(b'+') => {
            i += 1;
            if !d(i) {
                return false;
            }
            i += 1;
            if d(i) {
                i += 1;
            }
            if b.get(i) == Some(&b':') {
                if !(d(i + 1) && d(i + 2)) {
                    return false;
                }
                i += 3;
            }
        }
        _ => return false,
    }
    i == b.len()
}

/// PyYAML implicit resolution for a plain scalar (registration order per
/// first-char trigger set; falls through to `!!str`).
fn resolve_plain(value: &str) -> &'static str {
    let first = value.chars().next();
    let candidates: &[&str] = match first {
        None => &[TAG_NULL],
        Some(c) => match c {
            'y' | 'Y' | 't' | 'T' | 'f' | 'F' | 'o' | 'O' => &[TAG_BOOL],
            'n' | 'N' => &[TAG_BOOL, TAG_NULL],
            '-' | '+' => &[TAG_FLOAT, TAG_INT],
            '0'..='9' => &[TAG_FLOAT, TAG_INT, TAG_TIMESTAMP],
            '.' => &[TAG_FLOAT],
            '<' => &[TAG_MERGE],
            '~' => &[TAG_NULL],
            '=' => &[TAG_VALUE],
            _ => &[],
        },
    };
    for tag in candidates {
        let hit = match *tag {
            TAG_BOOL => match_bool(value),
            TAG_FLOAT => match_float(value),
            TAG_INT => match_int(value),
            TAG_MERGE => value == "<<",
            TAG_NULL => match_null(value),
            TAG_TIMESTAMP => match_timestamp(value),
            TAG_VALUE => value == "=",
            _ => false,
        };
        if hit {
            return tag;
        }
    }
    TAG_STR
}

// ---------------------------------------------------------------------------
// Reader (yaml/reader.py) — stream of chars with '\0' sentinel
// ---------------------------------------------------------------------------

fn yaml_printable(c: char) -> bool {
    matches!(c,
        '\t' | '\n' | '\r' | '\x20'..='\x7e' | '\u{85}'
        | '\u{a0}'..='\u{d7ff}' | '\u{e000}'..='\u{fffd}' | '\u{10000}'..='\u{10ffff}')
}

fn check_printable(data: &str) -> Result<(), YErr> {
    for (pos, c) in data.chars().enumerate() {
        // stdin surrogateescape sentinel: the oracle holds a lone surrogate
        // here (never yaml-printable) and reports ITS code point.
        if let Some(sur) = crate::pycompat::sentinel_surrogate(c) {
            return Err(YErr::Reader(format!(
                "unacceptable character #x{sur:04x}: special characters are not allowed\n  in \"<unicode string>\", position {pos}",
            )));
        }
        if !yaml_printable(c) {
            return Err(YErr::Reader(format!(
                "unacceptable character #x{:04x}: special characters are not allowed\n  in \"<unicode string>\", position {}",
                c as u32, pos
            )));
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Tokens (yaml/tokens.py)
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TK {
    StreamStart,
    StreamEnd,
    Directive,
    DocumentStart,
    DocumentEnd,
    BlockSequenceStart,
    BlockMappingStart,
    BlockEnd,
    FlowSequenceStart,
    FlowMappingStart,
    FlowSequenceEnd,
    FlowMappingEnd,
    BlockEntry,
    FlowEntry,
    Key,
    Value,
    Alias,
    Anchor,
    Tag,
    Scalar,
}

#[derive(Clone, Debug)]
enum DirectiveVal {
    Yaml { major_is_1: bool },
    Tag { handle: String, prefix: String },
    Other,
}

#[derive(Clone, Debug)]
struct Tok {
    kind: TK,
    /// Scalar value / anchor / alias name.
    value: String,
    plain: bool,
    /// Tag token: (handle, suffix).
    tag: Option<(Option<String>, String)>,
    directive: Option<DirectiveVal>,
}

impl Tok {
    fn simple(kind: TK) -> Tok {
        Tok {
            kind,
            value: String::new(),
            plain: false,
            tag: None,
            directive: None,
        }
    }

    /// `token.id` strings used inside parser error messages.
    fn id(&self) -> &'static str {
        match self.kind {
            TK::StreamStart => "<stream start>",
            TK::StreamEnd => "<stream end>",
            TK::Directive => "<directive>",
            TK::DocumentStart => "<document start>",
            TK::DocumentEnd => "<document end>",
            TK::BlockSequenceStart => "<block sequence start>",
            TK::BlockMappingStart => "<block mapping start>",
            TK::BlockEnd => "<block end>",
            TK::FlowSequenceStart => "[",
            TK::FlowMappingStart => "{",
            TK::FlowSequenceEnd => "]",
            TK::FlowMappingEnd => "}",
            TK::BlockEntry => "-",
            TK::FlowEntry => ",",
            TK::Key => "?",
            TK::Value => ":",
            TK::Alias => "<alias>",
            TK::Anchor => "<anchor>",
            TK::Tag => "<tag>",
            TK::Scalar => "<scalar>",
        }
    }
}

// ---------------------------------------------------------------------------
// Scanner (yaml/scanner.py) — faithful port; error strings verbatim
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
struct SimpleKey {
    token_number: usize,
    required: bool,
    index: usize,
    line: usize,
    column: i64,
}

struct Scanner {
    buf: Vec<char>, // input + '\0' sentinel
    pointer: usize,
    index: usize,
    line: usize,
    column: i64,
    done: bool,
    flow_level: usize,
    tokens: std::collections::VecDeque<Tok>,
    tokens_taken: usize,
    indent: i64,
    indents: Vec<i64>,
    allow_simple_key: bool,
    possible_simple_keys: HashMap<usize, SimpleKey>,
}

fn is_break(c: char) -> bool {
    matches!(c, '\r' | '\n' | '\u{85}' | '\u{2028}' | '\u{2029}')
}

fn is_z_ws_break(c: char) -> bool {
    matches!(c, '\0' | ' ' | '\t') || is_break(c)
}

fn is_z_break(c: char) -> bool {
    c == '\0' || is_break(c)
}

fn is_word_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '-' || c == '_'
}

fn repr_char(c: char) -> String {
    py_repr_str(&c.to_string())
}

impl Scanner {
    fn new(data: &str) -> Scanner {
        let mut buf: Vec<char> = data.chars().collect();
        buf.push('\0');
        let mut s = Scanner {
            buf,
            pointer: 0,
            index: 0,
            line: 0,
            column: 0,
            done: false,
            flow_level: 0,
            tokens: std::collections::VecDeque::new(),
            tokens_taken: 0,
            indent: -1,
            indents: Vec::new(),
            allow_simple_key: true,
            possible_simple_keys: HashMap::new(),
        };
        s.tokens.push_back(Tok::simple(TK::StreamStart));
        s
    }

    fn peek(&self, k: usize) -> char {
        self.buf.get(self.pointer + k).copied().unwrap_or('\0')
    }

    fn prefix(&self, l: usize) -> String {
        let end = (self.pointer + l).min(self.buf.len());
        self.buf[self.pointer..end].iter().collect()
    }

    fn forward(&mut self, l: usize) {
        for _ in 0..l {
            let ch = self.buf.get(self.pointer).copied().unwrap_or('\0');
            self.pointer += 1;
            self.index += 1;
            let next = self.buf.get(self.pointer).copied().unwrap_or('\0');
            if matches!(ch, '\n' | '\u{85}' | '\u{2028}' | '\u{2029}')
                || (ch == '\r' && next != '\n')
            {
                self.line += 1;
                self.column = 0;
            } else if ch != '\u{feff}' {
                self.column += 1;
            }
        }
    }

    fn skip_spaces(&mut self) {
        while self.peek(0) == ' ' {
            self.forward(1);
        }
    }

    // --- public token interface -------------------------------------------

    fn need_more_tokens(&mut self) -> Result<bool, YErr> {
        if self.done {
            return Ok(false);
        }
        if self.tokens.is_empty() {
            return Ok(true);
        }
        self.stale_possible_simple_keys()?;
        if self.next_possible_simple_key() == Some(self.tokens_taken) {
            return Ok(true);
        }
        Ok(false)
    }

    fn ensure(&mut self) -> Result<(), YErr> {
        while self.need_more_tokens()? {
            self.fetch_more_tokens()?;
        }
        Ok(())
    }

    fn check_token(&mut self, choices: &[TK]) -> Result<bool, YErr> {
        self.ensure()?;
        if let Some(t) = self.tokens.front() {
            if choices.is_empty() {
                return Ok(true);
            }
            return Ok(choices.contains(&t.kind));
        }
        Ok(false)
    }

    fn peek_token(&mut self) -> Result<Tok, YErr> {
        self.ensure()?;
        self.tokens
            .front()
            .cloned()
            .ok_or_else(|| YErr::Internal("IndexError: peek past stream end".to_string()))
    }

    fn get_token(&mut self) -> Result<Tok, YErr> {
        self.ensure()?;
        match self.tokens.pop_front() {
            Some(t) => {
                self.tokens_taken += 1;
                Ok(t)
            }
            None => Err(YErr::Internal("IndexError: get past stream end".to_string())),
        }
    }

    // --- simple keys --------------------------------------------------------

    fn next_possible_simple_key(&self) -> Option<usize> {
        self.possible_simple_keys
            .values()
            .map(|k| k.token_number)
            .min()
    }

    fn stale_possible_simple_keys(&mut self) -> Result<(), YErr> {
        let levels: Vec<usize> = self.possible_simple_keys.keys().copied().collect();
        for level in levels {
            let key = self.possible_simple_keys[&level];
            if key.line != self.line || self.index - key.index > 1024 {
                if key.required {
                    return Err(YErr::Marked("could not find expected ':'".to_string()));
                }
                self.possible_simple_keys.remove(&level);
            }
        }
        Ok(())
    }

    fn save_possible_simple_key(&mut self) -> Result<(), YErr> {
        let required = self.flow_level == 0 && self.indent == self.column;
        if self.allow_simple_key {
            self.remove_possible_simple_key()?;
            let token_number = self.tokens_taken + self.tokens.len();
            self.possible_simple_keys.insert(
                self.flow_level,
                SimpleKey {
                    token_number,
                    required,
                    index: self.index,
                    line: self.line,
                    column: self.column,
                },
            );
        }
        Ok(())
    }

    fn remove_possible_simple_key(&mut self) -> Result<(), YErr> {
        if let Some(key) = self.possible_simple_keys.get(&self.flow_level) {
            if key.required {
                return Err(YErr::Marked("could not find expected ':'".to_string()));
            }
            self.possible_simple_keys.remove(&self.flow_level);
        }
        Ok(())
    }

    // --- indentation --------------------------------------------------------

    fn unwind_indent(&mut self, column: i64) {
        if self.flow_level != 0 {
            return;
        }
        while self.indent > column {
            self.indent = self.indents.pop().unwrap_or(-1);
            self.tokens.push_back(Tok::simple(TK::BlockEnd));
        }
    }

    fn add_indent(&mut self, column: i64) -> bool {
        if self.indent < column {
            self.indents.push(self.indent);
            self.indent = column;
            true
        } else {
            false
        }
    }

    // --- fetchers -----------------------------------------------------------

    fn fetch_more_tokens(&mut self) -> Result<(), YErr> {
        self.scan_to_next_token();
        self.stale_possible_simple_keys()?;
        self.unwind_indent(self.column);
        let ch = self.peek(0);
        if ch == '\0' {
            return self.fetch_stream_end();
        }
        if ch == '%' && self.check_directive() {
            return self.fetch_directive();
        }
        if ch == '-' && self.check_document_indicator("---") {
            return self.fetch_document_indicator(TK::DocumentStart);
        }
        if ch == '.' && self.check_document_indicator("...") {
            return self.fetch_document_indicator(TK::DocumentEnd);
        }
        match ch {
            '[' => return self.fetch_flow_collection_start(TK::FlowSequenceStart),
            '{' => return self.fetch_flow_collection_start(TK::FlowMappingStart),
            ']' => return self.fetch_flow_collection_end(TK::FlowSequenceEnd),
            '}' => return self.fetch_flow_collection_end(TK::FlowMappingEnd),
            ',' => return self.fetch_flow_entry(),
            _ => {}
        }
        if ch == '-' && is_z_ws_break(self.peek(1)) {
            return self.fetch_block_entry();
        }
        if ch == '?' && (self.flow_level != 0 || is_z_ws_break(self.peek(1))) {
            return self.fetch_key();
        }
        if ch == ':' && (self.flow_level != 0 || is_z_ws_break(self.peek(1))) {
            return self.fetch_value();
        }
        if ch == '*' {
            return self.fetch_anchor_or_alias(TK::Alias);
        }
        if ch == '&' {
            return self.fetch_anchor_or_alias(TK::Anchor);
        }
        if ch == '!' {
            return self.fetch_tag();
        }
        if ch == '|' && self.flow_level == 0 {
            return self.fetch_block_scalar('|');
        }
        if ch == '>' && self.flow_level == 0 {
            return self.fetch_block_scalar('>');
        }
        if ch == '\'' {
            return self.fetch_flow_scalar('\'');
        }
        if ch == '"' {
            return self.fetch_flow_scalar('"');
        }
        if self.check_plain() {
            return self.fetch_plain();
        }
        Err(YErr::Marked(format!(
            "found character {} that cannot start any token",
            repr_char(ch)
        )))
    }

    fn check_directive(&self) -> bool {
        self.column == 0
    }

    fn check_document_indicator(&self, marker: &str) -> bool {
        self.column == 0 && self.prefix(3) == marker && is_z_ws_break(self.peek(3))
    }

    fn check_plain(&self) -> bool {
        let ch = self.peek(0);
        let starters = "-?:,[]{}#&*!|>'\"%@`";
        (!is_z_ws_break(ch) && !starters.contains(ch))
            || (!is_z_ws_break(self.peek(1))
                && (ch == '-' || (self.flow_level == 0 && (ch == '?' || ch == ':'))))
    }

    fn fetch_stream_end(&mut self) -> Result<(), YErr> {
        self.unwind_indent(-1);
        self.remove_possible_simple_key()?;
        self.allow_simple_key = false;
        self.possible_simple_keys.clear();
        self.tokens.push_back(Tok::simple(TK::StreamEnd));
        self.done = true;
        Ok(())
    }

    fn fetch_directive(&mut self) -> Result<(), YErr> {
        self.unwind_indent(-1);
        self.remove_possible_simple_key()?;
        self.allow_simple_key = false;
        let tok = self.scan_directive()?;
        self.tokens.push_back(tok);
        Ok(())
    }

    fn fetch_document_indicator(&mut self, kind: TK) -> Result<(), YErr> {
        self.unwind_indent(-1);
        self.remove_possible_simple_key()?;
        self.allow_simple_key = false;
        self.forward(3);
        self.tokens.push_back(Tok::simple(kind));
        Ok(())
    }

    fn fetch_flow_collection_start(&mut self, kind: TK) -> Result<(), YErr> {
        self.save_possible_simple_key()?;
        self.flow_level += 1;
        self.allow_simple_key = true;
        self.forward(1);
        self.tokens.push_back(Tok::simple(kind));
        Ok(())
    }

    fn fetch_flow_collection_end(&mut self, kind: TK) -> Result<(), YErr> {
        self.remove_possible_simple_key()?;
        self.flow_level = self.flow_level.saturating_sub(1);
        self.allow_simple_key = false;
        self.forward(1);
        self.tokens.push_back(Tok::simple(kind));
        Ok(())
    }

    fn fetch_flow_entry(&mut self) -> Result<(), YErr> {
        self.allow_simple_key = true;
        self.remove_possible_simple_key()?;
        self.forward(1);
        self.tokens.push_back(Tok::simple(TK::FlowEntry));
        Ok(())
    }

    fn fetch_block_entry(&mut self) -> Result<(), YErr> {
        if self.flow_level == 0 {
            if !self.allow_simple_key {
                return Err(YErr::Marked(
                    "sequence entries are not allowed here".to_string(),
                ));
            }
            if self.add_indent(self.column) {
                self.tokens.push_back(Tok::simple(TK::BlockSequenceStart));
            }
        }
        self.allow_simple_key = true;
        self.remove_possible_simple_key()?;
        self.forward(1);
        self.tokens.push_back(Tok::simple(TK::BlockEntry));
        Ok(())
    }

    fn fetch_key(&mut self) -> Result<(), YErr> {
        if self.flow_level == 0 {
            if !self.allow_simple_key {
                return Err(YErr::Marked(
                    "mapping keys are not allowed here".to_string(),
                ));
            }
            if self.add_indent(self.column) {
                self.tokens.push_back(Tok::simple(TK::BlockMappingStart));
            }
        }
        self.allow_simple_key = self.flow_level == 0;
        self.remove_possible_simple_key()?;
        self.forward(1);
        self.tokens.push_back(Tok::simple(TK::Key));
        Ok(())
    }

    fn fetch_value(&mut self) -> Result<(), YErr> {
        if let Some(key) = self.possible_simple_keys.remove(&self.flow_level) {
            let insert_at = key.token_number - self.tokens_taken;
            self.tokens.insert(insert_at, Tok::simple(TK::Key));
            if self.flow_level == 0 && self.add_indent(key.column) {
                self.tokens
                    .insert(insert_at, Tok::simple(TK::BlockMappingStart));
            }
            self.allow_simple_key = false;
        } else {
            if self.flow_level == 0 {
                if !self.allow_simple_key {
                    return Err(YErr::Marked(
                        "mapping values are not allowed here".to_string(),
                    ));
                }
                if self.add_indent(self.column) {
                    self.tokens.push_back(Tok::simple(TK::BlockMappingStart));
                }
            }
            self.allow_simple_key = self.flow_level == 0;
            self.remove_possible_simple_key()?;
        }
        self.forward(1);
        self.tokens.push_back(Tok::simple(TK::Value));
        Ok(())
    }

    fn fetch_anchor_or_alias(&mut self, kind: TK) -> Result<(), YErr> {
        self.save_possible_simple_key()?;
        self.allow_simple_key = false;
        let tok = self.scan_anchor(kind)?;
        self.tokens.push_back(tok);
        Ok(())
    }

    fn fetch_tag(&mut self) -> Result<(), YErr> {
        self.save_possible_simple_key()?;
        self.allow_simple_key = false;
        let tok = self.scan_tag()?;
        self.tokens.push_back(tok);
        Ok(())
    }

    fn fetch_block_scalar(&mut self, style: char) -> Result<(), YErr> {
        self.allow_simple_key = true;
        self.remove_possible_simple_key()?;
        let tok = self.scan_block_scalar(style)?;
        self.tokens.push_back(tok);
        Ok(())
    }

    fn fetch_flow_scalar(&mut self, style: char) -> Result<(), YErr> {
        self.save_possible_simple_key()?;
        self.allow_simple_key = false;
        let tok = self.scan_flow_scalar(style)?;
        self.tokens.push_back(tok);
        Ok(())
    }

    fn fetch_plain(&mut self) -> Result<(), YErr> {
        self.save_possible_simple_key()?;
        self.allow_simple_key = false;
        let tok = self.scan_plain()?;
        self.tokens.push_back(tok);
        Ok(())
    }

    // --- scanners -----------------------------------------------------------

    fn scan_to_next_token(&mut self) {
        if self.index == 0 && self.peek(0) == '\u{feff}' {
            self.forward(1);
        }
        let mut found = false;
        while !found {
            self.skip_spaces();
            if self.peek(0) == '#' {
                while !is_z_break(self.peek(0)) {
                    self.forward(1);
                }
            }
            if !self.scan_line_break().is_empty() {
                if self.flow_level == 0 {
                    self.allow_simple_key = true;
                }
            } else {
                found = true;
            }
        }
    }

    fn scan_line_break(&mut self) -> String {
        let ch = self.peek(0);
        if matches!(ch, '\r' | '\n' | '\u{85}') {
            if self.prefix(2) == "\r\n" {
                self.forward(2);
            } else {
                self.forward(1);
            }
            return "\n".to_string();
        } else if matches!(ch, '\u{2028}' | '\u{2029}') {
            self.forward(1);
            return ch.to_string();
        }
        String::new()
    }

    fn scan_directive(&mut self) -> Result<Tok, YErr> {
        self.forward(1);
        let name = self.scan_directive_name()?;
        let value = if name == "YAML" {
            let v = self.scan_yaml_directive_value()?;
            Some(DirectiveVal::Yaml { major_is_1: v })
        } else if name == "TAG" {
            let (handle, prefix) = self.scan_tag_directive_value()?;
            Some(DirectiveVal::Tag { handle, prefix })
        } else {
            while !is_z_break(self.peek(0)) {
                self.forward(1);
            }
            Some(DirectiveVal::Other)
        };
        self.scan_ignored_line()?;
        Ok(Tok {
            kind: TK::Directive,
            value: name,
            plain: false,
            tag: None,
            directive: value,
        })
    }

    fn scan_directive_name(&mut self) -> Result<String, YErr> {
        let mut length = 0;
        let mut ch = self.peek(length);
        while is_word_char(ch) {
            length += 1;
            ch = self.peek(length);
        }
        if length == 0 {
            return Err(YErr::Marked(format!(
                "expected alphabetic or numeric character, but found {}",
                repr_char(ch)
            )));
        }
        let value = self.prefix(length);
        self.forward(length);
        let ch = self.peek(0);
        if !(ch == '\0' || ch == ' ' || is_break(ch)) {
            return Err(YErr::Marked(format!(
                "expected alphabetic or numeric character, but found {}",
                repr_char(ch)
            )));
        }
        Ok(value)
    }

    fn scan_yaml_directive_value(&mut self) -> Result<bool, YErr> {
        self.skip_spaces();
        let major = self.scan_yaml_directive_number()?;
        if self.peek(0) != '.' {
            return Err(YErr::Marked(format!(
                "expected a digit or '.', but found {}",
                repr_char(self.peek(0))
            )));
        }
        self.forward(1);
        let _minor = self.scan_yaml_directive_number()?;
        let ch = self.peek(0);
        if !(ch == '\0' || ch == ' ' || is_break(ch)) {
            return Err(YErr::Marked(format!(
                "expected a digit or ' ', but found {}",
                repr_char(ch)
            )));
        }
        Ok(major == Some(1))
    }

    fn scan_yaml_directive_number(&mut self) -> Result<Option<u64>, YErr> {
        let ch = self.peek(0);
        if !ch.is_ascii_digit() {
            return Err(YErr::Marked(format!(
                "expected a digit, but found {}",
                repr_char(ch)
            )));
        }
        let mut length = 0;
        while self.peek(length).is_ascii_digit() {
            length += 1;
        }
        let text = self.prefix(length);
        self.forward(length);
        Ok(text.parse::<u64>().ok()) // None = larger than u64 (still != 1)
    }

    fn scan_tag_directive_value(&mut self) -> Result<(String, String), YErr> {
        self.skip_spaces();
        let handle = self.scan_tag_handle("directive")?;
        let ch = self.peek(0);
        if ch != ' ' {
            return Err(YErr::Marked(format!(
                "expected ' ', but found {}",
                repr_char(ch)
            )));
        }
        self.skip_spaces();
        let prefix = self.scan_tag_uri("directive")?;
        let ch = self.peek(0);
        if !(ch == '\0' || ch == ' ' || is_break(ch)) {
            return Err(YErr::Marked(format!(
                "expected ' ', but found {}",
                repr_char(ch)
            )));
        }
        Ok((handle, prefix))
    }

    fn scan_ignored_line(&mut self) -> Result<(), YErr> {
        self.skip_spaces();
        if self.peek(0) == '#' {
            while !is_z_break(self.peek(0)) {
                self.forward(1);
            }
        }
        let ch = self.peek(0);
        if !is_z_break(ch) {
            return Err(YErr::Marked(format!(
                "expected a comment or a line break, but found {}",
                repr_char(ch)
            )));
        }
        self.scan_line_break();
        Ok(())
    }

    fn scan_anchor(&mut self, kind: TK) -> Result<Tok, YErr> {
        self.forward(1);
        let mut length = 0;
        let mut ch = self.peek(length);
        while is_word_char(ch) {
            length += 1;
            ch = self.peek(length);
        }
        if length == 0 {
            return Err(YErr::Marked(format!(
                "expected alphabetic or numeric character, but found {}",
                repr_char(ch)
            )));
        }
        let value = self.prefix(length);
        self.forward(length);
        let ch = self.peek(0);
        if !(is_z_ws_break(ch) || "?:,]}%@`".contains(ch)) {
            return Err(YErr::Marked(format!(
                "expected alphabetic or numeric character, but found {}",
                repr_char(ch)
            )));
        }
        Ok(Tok {
            kind,
            value,
            plain: false,
            tag: None,
            directive: None,
        })
    }

    fn scan_tag(&mut self) -> Result<Tok, YErr> {
        let ch = self.peek(1);
        let (handle, suffix): (Option<String>, String);
        if ch == '<' {
            self.forward(2);
            let s = self.scan_tag_uri("tag")?;
            if self.peek(0) != '>' {
                return Err(YErr::Marked(format!(
                    "expected '>', but found {}",
                    repr_char(self.peek(0))
                )));
            }
            self.forward(1);
            handle = None;
            suffix = s;
        } else if is_z_ws_break(ch) {
            handle = None;
            suffix = "!".to_string();
            self.forward(1);
        } else {
            let mut length = 1;
            let mut c = ch;
            let mut use_handle = false;
            while !(c == '\0' || c == ' ' || is_break(c)) {
                if c == '!' {
                    use_handle = true;
                    break;
                }
                length += 1;
                c = self.peek(length);
            }
            if use_handle {
                handle = Some(self.scan_tag_handle("tag")?);
            } else {
                handle = Some("!".to_string());
                self.forward(1);
            }
            suffix = self.scan_tag_uri("tag")?;
        }
        let ch = self.peek(0);
        if !(ch == '\0' || ch == ' ' || is_break(ch)) {
            return Err(YErr::Marked(format!(
                "expected ' ', but found {}",
                repr_char(ch)
            )));
        }
        Ok(Tok {
            kind: TK::Tag,
            value: String::new(),
            plain: false,
            tag: Some((handle, suffix)),
            directive: None,
        })
    }

    fn scan_tag_handle(&mut self, _name: &str) -> Result<String, YErr> {
        let ch = self.peek(0);
        if ch != '!' {
            return Err(YErr::Marked(format!(
                "expected '!', but found {}",
                repr_char(ch)
            )));
        }
        let mut length = 1;
        let mut c = self.peek(length);
        if c != ' ' {
            while is_word_char(c) {
                length += 1;
                c = self.peek(length);
            }
            if c != '!' {
                self.forward(length);
                return Err(YErr::Marked(format!(
                    "expected '!', but found {}",
                    repr_char(c)
                )));
            }
            length += 1;
        }
        let value = self.prefix(length);
        self.forward(length);
        Ok(value)
    }

    fn scan_tag_uri(&mut self, name: &str) -> Result<String, YErr> {
        let mut chunks = String::new();
        let mut length = 0;
        let mut ch = self.peek(length);
        while ch.is_ascii_alphanumeric() || "-;/?:@&=+$,_.!~*'()[]%".contains(ch) {
            if ch == '%' {
                chunks.push_str(&self.prefix(length));
                self.forward(length);
                length = 0;
                chunks.push_str(&self.scan_uri_escapes(name)?);
            } else {
                length += 1;
            }
            ch = self.peek(length);
        }
        if length > 0 {
            chunks.push_str(&self.prefix(length));
            self.forward(length);
        }
        if chunks.is_empty() {
            return Err(YErr::Marked(format!(
                "expected URI, but found {}",
                repr_char(ch)
            )));
        }
        Ok(chunks)
    }

    fn scan_uri_escapes(&mut self, _name: &str) -> Result<String, YErr> {
        let mut codes: Vec<u8> = Vec::new();
        while self.peek(0) == '%' {
            self.forward(1);
            for k in 0..2 {
                if !self.peek(k).is_ascii_hexdigit() {
                    return Err(YErr::Marked(format!(
                        "expected URI escape sequence of 2 hexadecimal numbers, but found {}",
                        repr_char(self.peek(k))
                    )));
                }
            }
            let hex = self.prefix(2);
            codes.push(u8::from_str_radix(&hex, 16).unwrap_or(0));
            self.forward(2);
        }
        match String::from_utf8(codes.clone()) {
            Ok(s) => Ok(s),
            Err(e) => {
                // Python embeds str(UnicodeDecodeError). Reproduce the common
                // single-byte form.
                let pos = e.utf8_error().valid_up_to();
                let byte = codes.get(pos).copied().unwrap_or(0);
                let reason = if pos + 1 >= codes.len() && e.utf8_error().error_len().is_none() {
                    "unexpected end of data"
                } else if (0x80..0xc0).contains(&byte) {
                    "invalid start byte"
                } else {
                    "invalid continuation byte"
                };
                Err(YErr::Marked(format!(
                    "'utf-8' codec can't decode byte 0x{byte:02x} in position {pos}: {reason}"
                )))
            }
        }
    }

    fn scan_block_scalar(&mut self, style: char) -> Result<Tok, YErr> {
        let folded = style == '>';
        let mut chunks = String::new();
        self.forward(1);
        let (chomping, increment) = self.scan_block_scalar_indicators()?;
        self.scan_ignored_line()?;

        let mut min_indent = self.indent + 1;
        if min_indent < 1 {
            min_indent = 1;
        }
        let (mut breaks, indent) = if let Some(inc) = increment {
            let indent = min_indent + inc - 1;
            (self.scan_block_scalar_breaks(indent), indent)
        } else {
            let (breaks, max_indent) = self.scan_block_scalar_indentation();
            (breaks, min_indent.max(max_indent))
        };
        let mut line_break = String::new();

        while self.column == indent && self.peek(0) != '\0' {
            chunks.push_str(&breaks);
            let leading_non_space = !matches!(self.peek(0), ' ' | '\t');
            let mut length = 0;
            while !is_z_break(self.peek(length)) {
                length += 1;
            }
            chunks.push_str(&self.prefix(length));
            self.forward(length);
            line_break = self.scan_line_break();
            breaks = self.scan_block_scalar_breaks(indent);
            if self.column == indent && self.peek(0) != '\0' {
                if folded
                    && line_break == "\n"
                    && leading_non_space
                    && !matches!(self.peek(0), ' ' | '\t')
                {
                    if breaks.is_empty() {
                        chunks.push(' ');
                    }
                } else {
                    chunks.push_str(&line_break);
                }
            } else {
                break;
            }
        }

        if chomping != Some(false) {
            chunks.push_str(&line_break);
        }
        if chomping == Some(true) {
            chunks.push_str(&breaks);
        }
        Ok(Tok {
            kind: TK::Scalar,
            value: chunks,
            plain: false,
            tag: None,
            directive: None,
        })
    }

    fn scan_block_scalar_indicators(&mut self) -> Result<(Option<bool>, Option<i64>), YErr> {
        let mut chomping: Option<bool> = None;
        let mut increment: Option<i64> = None;
        let mut ch = self.peek(0);
        if ch == '+' || ch == '-' {
            chomping = Some(ch == '+');
            self.forward(1);
            ch = self.peek(0);
            if ch.is_ascii_digit() {
                let inc = ch.to_digit(10).unwrap() as i64;
                if inc == 0 {
                    return Err(YErr::Marked(
                        "expected indentation indicator in the range 1-9, but found 0"
                            .to_string(),
                    ));
                }
                increment = Some(inc);
                self.forward(1);
            }
        } else if ch.is_ascii_digit() {
            let inc = ch.to_digit(10).unwrap() as i64;
            if inc == 0 {
                return Err(YErr::Marked(
                    "expected indentation indicator in the range 1-9, but found 0".to_string(),
                ));
            }
            increment = Some(inc);
            self.forward(1);
            ch = self.peek(0);
            if ch == '+' || ch == '-' {
                chomping = Some(ch == '+');
                self.forward(1);
            }
        }
        let ch = self.peek(0);
        if !(ch == '\0' || ch == ' ' || is_break(ch)) {
            return Err(YErr::Marked(format!(
                "expected chomping or indentation indicators, but found {}",
                repr_char(ch)
            )));
        }
        Ok((chomping, increment))
    }

    fn scan_block_scalar_indentation(&mut self) -> (String, i64) {
        let mut chunks = String::new();
        let mut max_indent = 0i64;
        loop {
            let ch = self.peek(0);
            if !(ch == ' ' || is_break(ch)) {
                break;
            }
            if ch != ' ' {
                chunks.push_str(&self.scan_line_break());
            } else {
                self.forward(1);
                if self.column > max_indent {
                    max_indent = self.column;
                }
            }
        }
        (chunks, max_indent)
    }

    fn scan_block_scalar_breaks(&mut self, indent: i64) -> String {
        let mut chunks = String::new();
        while self.column < indent && self.peek(0) == ' ' {
            self.forward(1);
        }
        while is_break(self.peek(0)) {
            chunks.push_str(&self.scan_line_break());
            while self.column < indent && self.peek(0) == ' ' {
                self.forward(1);
            }
        }
        chunks
    }

    fn scan_flow_scalar(&mut self, style: char) -> Result<Tok, YErr> {
        let double = style == '"';
        let mut chunks = String::new();
        let quote = self.peek(0);
        self.forward(1);
        chunks.push_str(&self.scan_flow_scalar_non_spaces(double)?);
        while self.peek(0) != quote {
            chunks.push_str(&self.scan_flow_scalar_spaces()?);
            chunks.push_str(&self.scan_flow_scalar_non_spaces(double)?);
        }
        self.forward(1);
        Ok(Tok {
            kind: TK::Scalar,
            value: chunks,
            plain: false,
            tag: None,
            directive: None,
        })
    }

    fn scan_flow_scalar_non_spaces(&mut self, double: bool) -> Result<String, YErr> {
        let mut chunks = String::new();
        loop {
            let mut length = 0;
            while !matches!(self.peek(length), '\'' | '"' | '\\')
                && !is_z_ws_break(self.peek(length))
            {
                length += 1;
            }
            if length > 0 {
                chunks.push_str(&self.prefix(length));
                self.forward(length);
            }
            let ch = self.peek(0);
            if !double && ch == '\'' && self.peek(1) == '\'' {
                chunks.push('\'');
                self.forward(2);
            } else if (double && ch == '\'') || (!double && (ch == '"' || ch == '\\')) {
                chunks.push(ch);
                self.forward(1);
            } else if double && ch == '\\' {
                self.forward(1);
                let ch = self.peek(0);
                let simple: Option<&str> = match ch {
                    '0' => Some("\0"),
                    'a' => Some("\x07"),
                    'b' => Some("\x08"),
                    't' | '\t' => Some("\t"),
                    'n' => Some("\n"),
                    'v' => Some("\x0b"),
                    'f' => Some("\x0c"),
                    'r' => Some("\r"),
                    'e' => Some("\x1b"),
                    ' ' => Some(" "),
                    '"' => Some("\""),
                    '\\' => Some("\\"),
                    '/' => Some("/"),
                    'N' => Some("\u{85}"),
                    '_' => Some("\u{a0}"),
                    'L' => Some("\u{2028}"),
                    'P' => Some("\u{2029}"),
                    _ => None,
                };
                if let Some(s) = simple {
                    chunks.push_str(s);
                    self.forward(1);
                } else if matches!(ch, 'x' | 'u' | 'U') {
                    let length = match ch {
                        'x' => 2,
                        'u' => 4,
                        _ => 8,
                    };
                    self.forward(1);
                    for k in 0..length {
                        if !self.peek(k).is_ascii_hexdigit() {
                            return Err(YErr::Marked(format!(
                                "expected escape sequence of {} hexadecimal numbers, but found {}",
                                length,
                                repr_char(self.peek(k))
                            )));
                        }
                    }
                    let code = u32::from_str_radix(&self.prefix(length), 16).unwrap_or(0);
                    match char::from_u32(code) {
                        Some(c) => chunks.push(c),
                        None => {
                            if code > 0x10ffff {
                                // Python chr() raises ValueError -> crash.
                                return Err(YErr::Internal(
                                    "ValueError: chr() arg not in range(0x110000)".to_string(),
                                ));
                            }
                            // Lone surrogate: representable in a Python str,
                            // not in Rust. ORACLE DIVERGENCE.
                            return Err(YErr::Internal(format!(
                                "surrogate escape \\u{code:04x} not representable"
                            )));
                        }
                    }
                    self.forward(length);
                } else if is_break(ch) {
                    self.scan_line_break();
                    chunks.push_str(&self.scan_flow_scalar_breaks()?);
                } else {
                    return Err(YErr::Marked(format!(
                        "found unknown escape character {}",
                        repr_char(ch)
                    )));
                }
            } else {
                return Ok(chunks);
            }
        }
    }

    fn scan_flow_scalar_spaces(&mut self) -> Result<String, YErr> {
        let mut chunks = String::new();
        let mut length = 0;
        while matches!(self.peek(length), ' ' | '\t') {
            length += 1;
        }
        let whitespaces = self.prefix(length);
        self.forward(length);
        let ch = self.peek(0);
        if ch == '\0' {
            return Err(YErr::Marked("found unexpected end of stream".to_string()));
        } else if is_break(ch) {
            let line_break = self.scan_line_break();
            let breaks = self.scan_flow_scalar_breaks()?;
            if line_break != "\n" {
                chunks.push_str(&line_break);
            } else if breaks.is_empty() {
                chunks.push(' ');
            }
            chunks.push_str(&breaks);
        } else {
            chunks.push_str(&whitespaces);
        }
        Ok(chunks)
    }

    fn scan_flow_scalar_breaks(&mut self) -> Result<String, YErr> {
        let mut chunks = String::new();
        loop {
            let prefix = self.prefix(3);
            if (prefix == "---" || prefix == "...") && is_z_ws_break(self.peek(3)) {
                return Err(YErr::Marked(
                    "found unexpected document separator".to_string(),
                ));
            }
            while matches!(self.peek(0), ' ' | '\t') {
                self.forward(1);
            }
            if is_break(self.peek(0)) {
                chunks.push_str(&self.scan_line_break());
            } else {
                return Ok(chunks);
            }
        }
    }

    fn scan_plain(&mut self) -> Result<Tok, YErr> {
        let mut chunks = String::new();
        let indent = self.indent + 1;
        let mut spaces = String::new();
        loop {
            let mut length = 0;
            if self.peek(0) == '#' {
                break;
            }
            loop {
                let ch = self.peek(length);
                let stop = is_z_ws_break(ch)
                    || (ch == ':'
                        && (is_z_ws_break(self.peek(length + 1))
                            || (self.flow_level != 0
                                && ",[]{}".contains(self.peek(length + 1)))))
                    || (self.flow_level != 0 && ",?[]{}".contains(ch));
                if stop {
                    break;
                }
                length += 1;
            }
            if length == 0 {
                break;
            }
            self.allow_simple_key = false;
            chunks.push_str(&spaces);
            chunks.push_str(&self.prefix(length));
            self.forward(length);
            spaces = match self.scan_plain_spaces()? {
                Some(s) => s,
                None => break,
            };
            if spaces.is_empty()
                || self.peek(0) == '#'
                || (self.flow_level == 0 && self.column < indent)
            {
                break;
            }
        }
        Ok(Tok {
            kind: TK::Scalar,
            value: chunks,
            plain: true,
            tag: None,
            directive: None,
        })
    }

    /// Returns None where Python's `scan_plain_spaces` returns None (document
    /// separator ahead), Some(chunks) otherwise.
    fn scan_plain_spaces(&mut self) -> Result<Option<String>, YErr> {
        let mut chunks = String::new();
        let mut length = 0;
        while self.peek(length) == ' ' {
            length += 1;
        }
        let whitespaces = self.prefix(length);
        self.forward(length);
        let ch = self.peek(0);
        if is_break(ch) {
            let line_break = self.scan_line_break();
            self.allow_simple_key = true;
            let prefix = self.prefix(3);
            if (prefix == "---" || prefix == "...") && is_z_ws_break(self.peek(3)) {
                return Ok(None);
            }
            let mut breaks = String::new();
            loop {
                let c = self.peek(0);
                if !(c == ' ' || is_break(c)) {
                    break;
                }
                if c == ' ' {
                    self.forward(1);
                } else {
                    breaks.push_str(&self.scan_line_break());
                    let prefix = self.prefix(3);
                    if (prefix == "---" || prefix == "...") && is_z_ws_break(self.peek(3)) {
                        return Ok(None);
                    }
                }
            }
            if line_break != "\n" {
                chunks.push_str(&line_break);
            } else if breaks.is_empty() {
                chunks.push(' ');
            }
            chunks.push_str(&breaks);
        } else if !whitespaces.is_empty() {
            chunks.push_str(&whitespaces);
        }
        Ok(Some(chunks))
    }
}

// ---------------------------------------------------------------------------
// Parser (yaml/parser.py) — LL(1) state machine; error strings verbatim
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
enum Ev {
    StreamStart,
    StreamEnd,
    DocStart,
    DocEnd,
    Alias,
    Scalar {
        tag: Option<String>,
        implicit: (bool, bool),
        value: String,
        anchor: Option<String>,
    },
    SeqStart {
        tag: Option<String>,
        anchor: Option<String>,
    },
    SeqEnd,
    MapStart {
        tag: Option<String>,
        anchor: Option<String>,
    },
    MapEnd,
}

#[derive(Clone, Copy, Debug)]
enum St {
    StreamStart,
    ImplicitDocumentStart,
    DocumentStart,
    DocumentContent,
    DocumentEnd,
    BlockNode,
    BlockSequenceFirstEntry,
    BlockSequenceEntry,
    IndentlessSequenceEntry,
    BlockMappingFirstKey,
    BlockMappingKey,
    BlockMappingValue,
    FlowSequenceFirstEntry,
    FlowSequenceEntry { first: bool },
    FlowSequenceEntryMappingKey,
    FlowSequenceEntryMappingValue,
    FlowSequenceEntryMappingEnd,
    FlowMappingFirstKey,
    FlowMappingKey { first: bool },
    FlowMappingValue,
    FlowMappingEmptyValue,
}

struct Parser {
    sc: Scanner,
    current_event: Option<Ev>,
    yaml_version_seen: bool,
    tag_handles: HashMap<String, String>,
    states: Vec<St>,
    state: Option<St>,
}

fn default_tag_handles() -> HashMap<String, String> {
    let mut m = HashMap::new();
    m.insert("!".to_string(), "!".to_string());
    m.insert("!!".to_string(), "tag:yaml.org,2002:".to_string());
    m
}

impl Parser {
    fn new(sc: Scanner) -> Parser {
        Parser {
            sc,
            current_event: None,
            yaml_version_seen: false,
            tag_handles: HashMap::new(),
            states: Vec::new(),
            state: Some(St::StreamStart),
        }
    }

    fn produce(&mut self) -> Result<(), YErr> {
        if self.current_event.is_none() {
            if let Some(st) = self.state {
                let ev = self.step(st)?;
                self.current_event = Some(ev);
            }
        }
        Ok(())
    }

    fn peek_event(&mut self) -> Result<Option<&Ev>, YErr> {
        self.produce()?;
        Ok(self.current_event.as_ref())
    }

    fn get_event(&mut self) -> Result<Ev, YErr> {
        self.produce()?;
        self.current_event
            .take()
            .ok_or_else(|| YErr::Internal("StopIteration: no more events".to_string()))
    }

    fn step(&mut self, st: St) -> Result<Ev, YErr> {
        match st {
            St::StreamStart => {
                self.sc.get_token()?;
                self.state = Some(St::ImplicitDocumentStart);
                Ok(Ev::StreamStart)
            }
            St::ImplicitDocumentStart => {
                if !self
                    .sc
                    .check_token(&[TK::Directive, TK::DocumentStart, TK::StreamEnd])?
                {
                    self.tag_handles = default_tag_handles();
                    self.states.push(St::DocumentEnd);
                    self.state = Some(St::BlockNode);
                    Ok(Ev::DocStart)
                } else {
                    self.step(St::DocumentStart)
                }
            }
            St::DocumentStart => {
                while self.sc.check_token(&[TK::DocumentEnd])? {
                    self.sc.get_token()?;
                }
                if !self.sc.check_token(&[TK::StreamEnd])? {
                    self.process_directives()?;
                    if !self.sc.check_token(&[TK::DocumentStart])? {
                        let tok = self.sc.peek_token()?;
                        return Err(YErr::Marked(format!(
                            "expected '<document start>', but found {}",
                            py_repr_str(tok.id())
                        )));
                    }
                    self.sc.get_token()?;
                    self.states.push(St::DocumentEnd);
                    self.state = Some(St::DocumentContent);
                    Ok(Ev::DocStart)
                } else {
                    self.sc.get_token()?;
                    self.state = None;
                    Ok(Ev::StreamEnd)
                }
            }
            St::DocumentEnd => {
                if self.sc.check_token(&[TK::DocumentEnd])? {
                    self.sc.get_token()?;
                }
                self.state = Some(St::DocumentStart);
                Ok(Ev::DocEnd)
            }
            St::DocumentContent => {
                if self.sc.check_token(&[
                    TK::Directive,
                    TK::DocumentStart,
                    TK::DocumentEnd,
                    TK::StreamEnd,
                ])? {
                    self.state = Some(self.states.pop().ok_or_else(state_underflow)?);
                    Ok(empty_scalar())
                } else {
                    self.parse_node(true, false)
                }
            }
            St::BlockNode => self.parse_node(true, false),
            St::BlockSequenceFirstEntry => {
                self.sc.get_token()?;
                self.block_sequence_entry()
            }
            St::BlockSequenceEntry => self.block_sequence_entry(),
            St::IndentlessSequenceEntry => {
                if self.sc.check_token(&[TK::BlockEntry])? {
                    self.sc.get_token()?;
                    if !self.sc.check_token(&[
                        TK::BlockEntry,
                        TK::Key,
                        TK::Value,
                        TK::BlockEnd,
                    ])? {
                        self.states.push(St::IndentlessSequenceEntry);
                        return self.parse_node(true, false);
                    }
                    self.state = Some(St::IndentlessSequenceEntry);
                    return Ok(empty_scalar());
                }
                self.state = Some(self.states.pop().ok_or_else(state_underflow)?);
                Ok(Ev::SeqEnd)
            }
            St::BlockMappingFirstKey => {
                self.sc.get_token()?;
                self.block_mapping_key()
            }
            St::BlockMappingKey => self.block_mapping_key(),
            St::BlockMappingValue => {
                if self.sc.check_token(&[TK::Value])? {
                    self.sc.get_token()?;
                    if !self.sc.check_token(&[TK::Key, TK::Value, TK::BlockEnd])? {
                        self.states.push(St::BlockMappingKey);
                        return self.parse_node(true, true);
                    }
                    self.state = Some(St::BlockMappingKey);
                    return Ok(empty_scalar());
                }
                self.state = Some(St::BlockMappingKey);
                Ok(empty_scalar())
            }
            St::FlowSequenceFirstEntry => {
                self.sc.get_token()?;
                self.flow_sequence_entry(true)
            }
            St::FlowSequenceEntry { first } => self.flow_sequence_entry(first),
            St::FlowSequenceEntryMappingKey => {
                self.sc.get_token()?;
                if !self
                    .sc
                    .check_token(&[TK::Value, TK::FlowEntry, TK::FlowSequenceEnd])?
                {
                    self.states.push(St::FlowSequenceEntryMappingValue);
                    return self.parse_node(false, false);
                }
                self.state = Some(St::FlowSequenceEntryMappingValue);
                Ok(empty_scalar())
            }
            St::FlowSequenceEntryMappingValue => {
                if self.sc.check_token(&[TK::Value])? {
                    self.sc.get_token()?;
                    if !self.sc.check_token(&[TK::FlowEntry, TK::FlowSequenceEnd])? {
                        self.states.push(St::FlowSequenceEntryMappingEnd);
                        return self.parse_node(false, false);
                    }
                    self.state = Some(St::FlowSequenceEntryMappingEnd);
                    return Ok(empty_scalar());
                }
                self.state = Some(St::FlowSequenceEntryMappingEnd);
                Ok(empty_scalar())
            }
            St::FlowSequenceEntryMappingEnd => {
                self.state = Some(St::FlowSequenceEntry { first: false });
                Ok(Ev::MapEnd)
            }
            St::FlowMappingFirstKey => {
                self.sc.get_token()?;
                self.flow_mapping_key(true)
            }
            St::FlowMappingKey { first } => self.flow_mapping_key(first),
            St::FlowMappingValue => {
                if self.sc.check_token(&[TK::Value])? {
                    self.sc.get_token()?;
                    if !self.sc.check_token(&[TK::FlowEntry, TK::FlowMappingEnd])? {
                        self.states.push(St::FlowMappingKey { first: false });
                        return self.parse_node(false, false);
                    }
                    self.state = Some(St::FlowMappingKey { first: false });
                    return Ok(empty_scalar());
                }
                self.state = Some(St::FlowMappingKey { first: false });
                Ok(empty_scalar())
            }
            St::FlowMappingEmptyValue => {
                self.state = Some(St::FlowMappingKey { first: false });
                Ok(empty_scalar())
            }
        }
    }

    fn block_sequence_entry(&mut self) -> Result<Ev, YErr> {
        if self.sc.check_token(&[TK::BlockEntry])? {
            self.sc.get_token()?;
            if !self.sc.check_token(&[TK::BlockEntry, TK::BlockEnd])? {
                self.states.push(St::BlockSequenceEntry);
                return self.parse_node(true, false);
            }
            self.state = Some(St::BlockSequenceEntry);
            return Ok(empty_scalar());
        }
        if !self.sc.check_token(&[TK::BlockEnd])? {
            let tok = self.sc.peek_token()?;
            return Err(YErr::Marked(format!(
                "expected <block end>, but found {}",
                py_repr_str(tok.id())
            )));
        }
        self.sc.get_token()?;
        self.state = Some(self.states.pop().ok_or_else(state_underflow)?);
        Ok(Ev::SeqEnd)
    }

    fn block_mapping_key(&mut self) -> Result<Ev, YErr> {
        if self.sc.check_token(&[TK::Key])? {
            self.sc.get_token()?;
            if !self.sc.check_token(&[TK::Key, TK::Value, TK::BlockEnd])? {
                self.states.push(St::BlockMappingValue);
                return self.parse_node(true, true);
            }
            self.state = Some(St::BlockMappingValue);
            return Ok(empty_scalar());
        }
        if !self.sc.check_token(&[TK::BlockEnd])? {
            let tok = self.sc.peek_token()?;
            return Err(YErr::Marked(format!(
                "expected <block end>, but found {}",
                py_repr_str(tok.id())
            )));
        }
        self.sc.get_token()?;
        self.state = Some(self.states.pop().ok_or_else(state_underflow)?);
        Ok(Ev::MapEnd)
    }

    fn flow_sequence_entry(&mut self, first: bool) -> Result<Ev, YErr> {
        if !self.sc.check_token(&[TK::FlowSequenceEnd])? {
            if !first {
                if self.sc.check_token(&[TK::FlowEntry])? {
                    self.sc.get_token()?;
                } else {
                    let tok = self.sc.peek_token()?;
                    return Err(YErr::Marked(format!(
                        "expected ',' or ']', but got {}",
                        py_repr_str(tok.id())
                    )));
                }
            }
            if self.sc.check_token(&[TK::Key])? {
                self.state = Some(St::FlowSequenceEntryMappingKey);
                return Ok(Ev::MapStart {
                    tag: None,
                    anchor: None,
                });
            } else if !self.sc.check_token(&[TK::FlowSequenceEnd])? {
                self.states.push(St::FlowSequenceEntry { first: false });
                return self.parse_node(false, false);
            }
        }
        self.sc.get_token()?;
        self.state = Some(self.states.pop().ok_or_else(state_underflow)?);
        Ok(Ev::SeqEnd)
    }

    fn flow_mapping_key(&mut self, first: bool) -> Result<Ev, YErr> {
        if !self.sc.check_token(&[TK::FlowMappingEnd])? {
            if !first {
                if self.sc.check_token(&[TK::FlowEntry])? {
                    self.sc.get_token()?;
                } else {
                    let tok = self.sc.peek_token()?;
                    return Err(YErr::Marked(format!(
                        "expected ',' or '}}', but got {}",
                        py_repr_str(tok.id())
                    )));
                }
            }
            if self.sc.check_token(&[TK::Key])? {
                self.sc.get_token()?;
                if !self
                    .sc
                    .check_token(&[TK::Value, TK::FlowEntry, TK::FlowMappingEnd])?
                {
                    self.states.push(St::FlowMappingValue);
                    return self.parse_node(false, false);
                }
                self.state = Some(St::FlowMappingValue);
                return Ok(empty_scalar());
            } else if !self.sc.check_token(&[TK::FlowMappingEnd])? {
                self.states.push(St::FlowMappingEmptyValue);
                return self.parse_node(false, false);
            }
        }
        self.sc.get_token()?;
        self.state = Some(self.states.pop().ok_or_else(state_underflow)?);
        Ok(Ev::MapEnd)
    }

    fn process_directives(&mut self) -> Result<(), YErr> {
        self.yaml_version_seen = false;
        self.tag_handles = HashMap::new();
        while self.sc.check_token(&[TK::Directive])? {
            let tok = self.sc.get_token()?;
            match tok.directive {
                Some(DirectiveVal::Yaml { major_is_1 }) => {
                    if self.yaml_version_seen {
                        return Err(YErr::Marked("found duplicate YAML directive".to_string()));
                    }
                    if !major_is_1 {
                        return Err(YErr::Marked(
                            "found incompatible YAML document (version 1.* is required)"
                                .to_string(),
                        ));
                    }
                    self.yaml_version_seen = true;
                }
                Some(DirectiveVal::Tag { handle, prefix }) => {
                    if self.tag_handles.contains_key(&handle) {
                        return Err(YErr::Marked(format!(
                            "duplicate tag handle {}",
                            py_repr_str(&handle)
                        )));
                    }
                    self.tag_handles.insert(handle, prefix);
                }
                _ => {}
            }
        }
        for (k, v) in default_tag_handles() {
            self.tag_handles.entry(k).or_insert(v);
        }
        Ok(())
    }

    fn parse_node(&mut self, block: bool, indentless_sequence: bool) -> Result<Ev, YErr> {
        if self.sc.check_token(&[TK::Alias])? {
            self.sc.get_token()?;
            self.state = Some(self.states.pop().ok_or_else(state_underflow)?);
            return Ok(Ev::Alias);
        }
        let mut anchor: Option<String> = None;
        let mut tag: Option<(Option<String>, String)> = None;
        let mut saw_properties = false;
        if self.sc.check_token(&[TK::Anchor])? {
            let tok = self.sc.get_token()?;
            anchor = Some(tok.value);
            saw_properties = true;
            if self.sc.check_token(&[TK::Tag])? {
                let tok = self.sc.get_token()?;
                tag = tok.tag;
            }
        } else if self.sc.check_token(&[TK::Tag])? {
            let tok = self.sc.get_token()?;
            tag = tok.tag;
            saw_properties = true;
            if self.sc.check_token(&[TK::Anchor])? {
                let tok = self.sc.get_token()?;
                anchor = Some(tok.value);
            }
        }
        let resolved_tag: Option<String> = match tag {
            Some((Some(handle), suffix)) => match self.tag_handles.get(&handle) {
                Some(prefix) => Some(format!("{prefix}{suffix}")),
                None => {
                    return Err(YErr::Marked(format!(
                        "found undefined tag handle {}",
                        py_repr_str(&handle)
                    )));
                }
            },
            Some((None, suffix)) => Some(suffix),
            None => None,
        };
        let implicit = resolved_tag.is_none() || resolved_tag.as_deref() == Some("!");
        if indentless_sequence && self.sc.check_token(&[TK::BlockEntry])? {
            self.state = Some(St::IndentlessSequenceEntry);
            return Ok(Ev::SeqStart {
                tag: resolved_tag,
                anchor,
            });
        }
        if self.sc.check_token(&[TK::Scalar])? {
            let tok = self.sc.get_token()?;
            let implicit_pair = if (tok.plain && resolved_tag.is_none())
                || resolved_tag.as_deref() == Some("!")
            {
                (true, false)
            } else if resolved_tag.is_none() {
                (false, true)
            } else {
                (false, false)
            };
            self.state = Some(self.states.pop().ok_or_else(state_underflow)?);
            return Ok(Ev::Scalar {
                tag: resolved_tag,
                implicit: implicit_pair,
                value: tok.value,
                anchor,
            });
        }
        if self.sc.check_token(&[TK::FlowSequenceStart])? {
            self.state = Some(St::FlowSequenceFirstEntry);
            return Ok(Ev::SeqStart {
                tag: resolved_tag,
                anchor,
            });
        }
        if self.sc.check_token(&[TK::FlowMappingStart])? {
            self.state = Some(St::FlowMappingFirstKey);
            return Ok(Ev::MapStart {
                tag: resolved_tag,
                anchor,
            });
        }
        if block && self.sc.check_token(&[TK::BlockSequenceStart])? {
            self.state = Some(St::BlockSequenceFirstEntry);
            return Ok(Ev::SeqStart {
                tag: resolved_tag,
                anchor,
            });
        }
        if block && self.sc.check_token(&[TK::BlockMappingStart])? {
            self.state = Some(St::BlockMappingFirstKey);
            return Ok(Ev::MapStart {
                tag: resolved_tag,
                anchor,
            });
        }
        if saw_properties {
            self.state = Some(self.states.pop().ok_or_else(state_underflow)?);
            return Ok(Ev::Scalar {
                tag: resolved_tag,
                implicit: (implicit, false),
                value: String::new(),
                anchor,
            });
        }
        // Context string ("while parsing a block/flow node") unused — only
        // the `problem` field reaches output.
        let tok = self.sc.peek_token()?;
        Err(YErr::Marked(format!(
            "expected the node content, but found {}",
            py_repr_str(tok.id())
        )))
    }
}

fn empty_scalar() -> Ev {
    Ev::Scalar {
        tag: None,
        implicit: (true, false),
        value: String::new(),
        anchor: None,
    }
}

fn state_underflow() -> YErr {
    YErr::Internal("IndexError: pop from empty parser state stack".to_string())
}

// ---------------------------------------------------------------------------
// Composer (yaml/composer.py) with the _BoundedLoader guards
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
enum Node {
    Scalar { tag: String, value: String },
    Seq { tag: String, items: Vec<Node> },
    Map { tag: String, pairs: Vec<(Node, Node)> },
}

impl Node {
    fn tag(&self) -> &str {
        match self {
            Node::Scalar { tag, .. } | Node::Seq { tag, .. } | Node::Map { tag, .. } => tag,
        }
    }

    fn id(&self) -> &'static str {
        match self {
            Node::Scalar { .. } => "scalar",
            Node::Seq { .. } => "sequence",
            Node::Map { .. } => "mapping",
        }
    }
}

/// PyYAML node class names, as CPython prints them in unpack TypeErrors.
fn py_node_class(n: &Node) -> &'static str {
    match n {
        Node::Scalar { .. } => "ScalarNode",
        Node::Seq { .. } => "SequenceNode",
        Node::Map { .. } => "MappingNode",
    }
}

struct Composer {
    p: Parser,
    anchors: std::collections::HashSet<String>,
    depth: usize,
}

impl Composer {
    fn get_single_node(&mut self) -> Result<Option<Node>, YErr> {
        // Drop STREAM-START.
        self.p.get_event()?;
        let mut document = None;
        if !matches!(self.p.peek_event()?, Some(Ev::StreamEnd)) {
            document = Some(self.compose_document()?);
        }
        if !matches!(self.p.peek_event()?, Some(Ev::StreamEnd)) {
            return Err(YErr::Marked("but found another document".to_string()));
        }
        self.p.get_event()?;
        Ok(document)
    }

    fn compose_document(&mut self) -> Result<Node, YErr> {
        self.p.get_event()?; // DOCUMENT-START
        let node = self.compose_node()?;
        self.p.get_event()?; // DOCUMENT-END
        self.anchors.clear();
        Ok(node)
    }

    /// `_BoundedLoader.compose_node`: alias rejection, then the node-count
    /// depth cap (root = 1; every scalar/sequence/mapping counts one level).
    fn compose_node(&mut self) -> Result<Node, YErr> {
        if matches!(self.p.peek_event()?, Some(Ev::Alias)) {
            return Err(YErr::Marked(
                "YAML aliases are not permitted in frontmatter".to_string(),
            ));
        }
        self.depth += 1;
        if self.depth > MAX_FRONTMATTER_DEPTH {
            self.depth -= 1;
            return Err(YErr::Marked(format!(
                "frontmatter nesting exceeds the {MAX_FRONTMATTER_DEPTH}-level cap"
            )));
        }
        let result = self.compose_node_inner();
        self.depth -= 1;
        result
    }

    fn compose_node_inner(&mut self) -> Result<Node, YErr> {
        // Duplicate-anchor check (aliases already rejected above).
        let anchor: Option<String> = match self.p.peek_event()? {
            Some(Ev::Scalar { anchor, .. })
            | Some(Ev::SeqStart { anchor, .. })
            | Some(Ev::MapStart { anchor, .. }) => anchor.clone(),
            _ => None,
        };
        if let Some(a) = &anchor {
            if self.anchors.contains(a) {
                return Err(YErr::Marked("second occurrence".to_string()));
            }
            self.anchors.insert(a.clone());
        }
        match self.p.peek_event()? {
            Some(Ev::Scalar { .. }) => {
                if let Ev::Scalar {
                    tag,
                    implicit,
                    value,
                    ..
                } = self.p.get_event()?
                {
                    let tag = match tag.as_deref() {
                        None | Some("!") => {
                            if implicit.0 {
                                resolve_plain(&value).to_string()
                            } else {
                                TAG_STR.to_string()
                            }
                        }
                        Some(t) => t.to_string(),
                    };
                    Ok(Node::Scalar { tag, value })
                } else {
                    unreachable!()
                }
            }
            Some(Ev::SeqStart { .. }) => {
                let tag = if let Ev::SeqStart { tag, .. } = self.p.get_event()? {
                    match tag.as_deref() {
                        None | Some("!") => TAG_SEQ.to_string(),
                        Some(t) => t.to_string(),
                    }
                } else {
                    unreachable!()
                };
                let mut items = Vec::new();
                while !matches!(self.p.peek_event()?, Some(Ev::SeqEnd)) {
                    items.push(self.compose_node()?);
                }
                self.p.get_event()?;
                Ok(Node::Seq { tag, items })
            }
            Some(Ev::MapStart { .. }) => {
                let tag = if let Ev::MapStart { tag, .. } = self.p.get_event()? {
                    match tag.as_deref() {
                        None | Some("!") => TAG_MAP.to_string(),
                        Some(t) => t.to_string(),
                    }
                } else {
                    unreachable!()
                };
                let mut pairs = Vec::new();
                while !matches!(self.p.peek_event()?, Some(Ev::MapEnd)) {
                    let key = self.compose_node()?;
                    let value = self.compose_node()?;
                    pairs.push((key, value));
                }
                self.p.get_event()?;
                Ok(Node::Map { tag, pairs })
            }
            _ => Err(YErr::Internal(
                "UnboundLocalError: compose_node on non-node event".to_string(),
            )),
        }
    }
}

// ---------------------------------------------------------------------------
// Constructor (yaml/constructor.py SafeConstructor + _no_duplicates)
// ---------------------------------------------------------------------------

fn construct_scalar_value(node: &Node) -> Result<String, YErr> {
    if let Node::Map { pairs, .. } = node {
        for (k, v) in pairs {
            if k.tag() == TAG_VALUE {
                return construct_scalar_value(v);
            }
        }
    }
    match node {
        Node::Scalar { value, .. } => Ok(value.clone()),
        _ => Err(YErr::Marked(format!(
            "expected a scalar node, but found {}",
            node.id()
        ))),
    }
}

fn construct_yaml_bool(node: &Node) -> Result<Yaml, YErr> {
    let value = construct_scalar_value(node)?;
    match value.to_lowercase().as_str() {
        "yes" | "true" | "on" => Ok(Yaml::Bool(true)),
        "no" | "false" | "off" => Ok(Yaml::Bool(false)),
        // Oracle: KeyError out of bool_values (uncaught crash).
        other => Err(YErr::Internal(format!(
            "KeyError: {}",
            py_repr_str(other)
        ))),
    }
}

/// CPython `int(s, base)` for base 2/8/10/16 over the strings PyYAML's int
/// constructor produces: Unicode strip, one optional sign, an optional
/// matching base prefix (`0b`/`0B`, `0o`/`0O`, `0x`/`0X` — never for base
/// 10), single underscores strictly between digits (or right after the
/// prefix), and — base 10 only — the 4300-digit conversion limit, checked on
/// the leading digit span before trailing junk is diagnosed (matches CPython
/// order: `int('9'*4301 + 'z')` reports the limit, not the literal).
/// Returns (negative, magnitude). Invalid input mirrors the oracle's uncaught
/// `ValueError` (decision 3).
/// CPython also accepts non-ASCII `Nd` decimal digits
/// (`int('٥') == 5`); those still report invalid-literal here.
fn py_int_parse(s: &str, base: u32) -> Result<(bool, Mag), YErr> {
    let invalid = || {
        YErr::Internal(format!(
            "ValueError: invalid literal for int() with base {}: {}",
            base,
            py_repr_str(s)
        ))
    };
    let t = py_strip(s);
    let chars: Vec<char> = t.chars().collect();
    let mut i = 0;
    let neg = match chars.first() {
        Some('-') => {
            i = 1;
            true
        }
        Some('+') => {
            i = 1;
            false
        }
        _ => false,
    };
    // Optional base prefix (only the one matching `base`).
    let prefix = match base {
        2 => Some(('b', 'B')),
        8 => Some(('o', 'O')),
        16 => Some(('x', 'X')),
        _ => None,
    };
    if let Some((lo, hi)) = prefix {
        if chars.get(i) == Some(&'0') && matches!(chars.get(i + 1), Some(c) if *c == lo || *c == hi)
        {
            i += 2;
            // A single underscore may follow the prefix.
            if chars.get(i) == Some(&'_') && chars.get(i + 1).is_some_and(|c| c.is_digit(base)) {
                i += 1;
            }
        }
    }
    // The 4300-digit limit is checked on the leading digit/underscore span.
    if base == 10 {
        let span_digits = chars[i..]
            .iter()
            .take_while(|c| c.is_ascii_digit() || **c == '_')
            .filter(|c| c.is_ascii_digit())
            .count();
        if span_digits > INT_MAX_STR_DIGITS {
            return Err(int_parse_limit_err(span_digits));
        }
    }
    let mut mag = Mag::zero();
    let mut prev_digit = false;
    let mut ndigits = 0usize;
    while i < chars.len() {
        let c = chars[i];
        if c == '_' {
            if !prev_digit || !chars.get(i + 1).is_some_and(|c| c.is_digit(base)) {
                return Err(invalid());
            }
            prev_digit = false;
        } else if let Some(d) = c.to_digit(base) {
            if !c.is_ascii() {
                // Unicode digits: SEAM above (to_digit is ASCII-only anyway).
                return Err(invalid());
            }
            mag.mul_add_small(base as u64, d as u64);
            prev_digit = true;
            ndigits += 1;
        } else {
            return Err(invalid());
        }
        i += 1;
    }
    if ndigits == 0 {
        return Err(invalid());
    }
    Ok((neg, mag))
}

fn construct_yaml_int(node: &Node) -> Result<Yaml, YErr> {
    let raw = construct_scalar_value(node)?;
    let value = raw.replace('_', "");
    if value.is_empty() {
        // Oracle: IndexError on value[0] (uncaught crash).
        return Err(YErr::Internal(
            "IndexError: string index out of range".to_string(),
        ));
    }
    let mut neg = false;
    let mut v = value.as_str();
    if v.starts_with('-') {
        neg = true;
        v = &v[1..];
    } else if v.starts_with('+') {
        v = &v[1..];
    }
    if v == "0" {
        return Ok(Yaml::Int(0));
    }
    if let Some(rest) = v.strip_prefix("0b") {
        let (pneg, mag) = py_int_parse(rest, 2)?;
        return Ok(yaml_int(neg ^ pneg, &mag));
    }
    if let Some(rest) = v.strip_prefix("0x") {
        let (pneg, mag) = py_int_parse(rest, 16)?;
        return Ok(yaml_int(neg ^ pneg, &mag));
    }
    if v.starts_with('0') {
        let (pneg, mag) = py_int_parse(v, 8)?;
        return Ok(yaml_int(neg ^ pneg, &mag));
    }
    if v.contains(':') {
        // digits.reverse() + base*=60 in PyYAML == Horner in document order.
        // Parts go through full int(part), so each may carry its own sign.
        let mut acc_neg = false;
        let mut acc = Mag::zero();
        for part in v.split(':') {
            let (pneg, pmag) = py_int_parse(part, 10)?;
            acc.mul_add_small(60, 0);
            if acc_neg == pneg || pmag.is_zero() {
                acc.add(&pmag);
            } else {
                match acc.cmp_mag(&pmag) {
                    std::cmp::Ordering::Less => {
                        let mut m = pmag.clone();
                        m.sub(&acc);
                        acc = m;
                        acc_neg = pneg;
                    }
                    std::cmp::Ordering::Equal => {
                        acc = Mag::zero();
                        acc_neg = false;
                    }
                    std::cmp::Ordering::Greater => acc.sub(&pmag),
                }
            }
            if acc.is_zero() {
                acc_neg = false;
            }
        }
        return Ok(yaml_int(neg ^ acc_neg, &acc));
    }
    let (pneg, mag) = py_int_parse(v, 10)?;
    Ok(yaml_int(neg ^ pneg, &mag))
}

fn construct_yaml_float(node: &Node) -> Result<Yaml, YErr> {
    let raw = construct_scalar_value(node)?;
    let value = raw.replace('_', "").to_lowercase();
    if value.is_empty() {
        return Err(YErr::Internal(
            "IndexError: string index out of range".to_string(),
        ));
    }
    let mut sign = 1.0f64;
    let mut v = value.as_str();
    if v.starts_with('-') {
        sign = -1.0;
        v = &v[1..];
    } else if v.starts_with('+') {
        v = &v[1..];
    }
    if v == ".inf" {
        return Ok(Yaml::Float(sign * f64::INFINITY));
    }
    if v == ".nan" {
        // PyYAML returns the shared nan_value object; sign ignored.
        return Ok(Yaml::Float(f64::NAN));
    }
    if v.contains(':') {
        let mut digits: Vec<f64> = Vec::new();
        for part in v.split(':') {
            digits.push(py_float_parse(part)?);
        }
        digits.reverse();
        let mut base = 1.0f64;
        let mut acc = 0.0f64;
        for d in digits {
            acc += d * base;
            base *= 60.0;
        }
        return Ok(Yaml::Float(sign * acc));
    }
    Ok(Yaml::Float(sign * py_float_parse(v)?))
}

/// Python `float(str)`: Unicode-strip, then the usual grammar incl.
/// `inf`/`infinity`/`nan` (input is already lowercased by the caller).
fn py_float_parse(s: &str) -> Result<f64, YErr> {
    let t = py_strip(s);
    match t.parse::<f64>() {
        Ok(v) => Ok(v),
        Err(_) => Err(YErr::Internal(format!(
            "ValueError: could not convert string to float: {}",
            py_repr_str(s)
        ))),
    }
}

fn construct_yaml_binary(node: &Node) -> Result<Yaml, YErr> {
    let value = construct_scalar_value(node)?;
    if let Some((pos, c)) = value.chars().enumerate().find(|(_, c)| !c.is_ascii()) {
        let cp = c as u32;
        let esc = if cp < 0x100 {
            format!("\\x{cp:02x}")
        } else if cp < 0x10000 {
            format!("\\u{cp:04x}")
        } else {
            format!("\\U{cp:08x}")
        };
        return Err(YErr::Marked(format!(
            "failed to convert base64 data into ascii: 'ascii' codec can't encode character '{esc}' in position {pos}: ordinal not in range(128)"
        )));
    }
    match decode_base64_lenient(value.as_bytes()) {
        Ok(bytes) => Ok(Yaml::Bytes(bytes)),
        Err(msg) => Err(YErr::Marked(format!(
            "failed to decode base64 data: {msg}"
        ))),
    }
}

/// `base64.decodebytes` (binascii a2b_base64, non-strict): non-alphabet
/// characters are skipped; padding after >=2 quad chars terminates decode.
/// Error-message coverage is limited to the common forms.
fn decode_base64_lenient(data: &[u8]) -> Result<Vec<u8>, String> {
    fn val(b: u8) -> Option<u32> {
        match b {
            b'A'..=b'Z' => Some((b - b'A') as u32),
            b'a'..=b'z' => Some((b - b'a' + 26) as u32),
            b'0'..=b'9' => Some((b - b'0' + 52) as u32),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    }
    let mut out = Vec::new();
    let mut acc: u32 = 0;
    let mut quad = 0usize;
    let mut ndata = 0usize;
    for &b in data {
        if b == b'=' && quad >= 2 {
            match quad {
                2 => out.push((acc >> 4) as u8),
                3 => {
                    out.push((acc >> 10) as u8);
                    out.push(((acc >> 2) & 0xff) as u8);
                }
                _ => {}
            }
            return Ok(out);
        }
        if let Some(v) = val(b) {
            acc = (acc << 6) | v;
            quad += 1;
            ndata += 1;
            if quad == 4 {
                out.push((acc >> 16) as u8);
                out.push(((acc >> 8) & 0xff) as u8);
                out.push((acc & 0xff) as u8);
                acc = 0;
                quad = 0;
            }
        }
    }
    match quad {
        0 => Ok(out),
        1 => Err(format!(
            "Invalid base64-encoded string: number of data characters ({ndata}) cannot be 1 more than a multiple of 4"
        )),
        _ => Err("Incorrect padding".to_string()),
    }
}

fn days_in_month(year: i64, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if (year % 4 == 0 && year % 100 != 0) || year % 400 == 0 {
                29
            } else {
                28
            }
        }
        _ => 0,
    }
}

/// The *constructor's* timestamp regexp (more lenient than the resolver's:
/// 1-2 digit month/day in the date-only form too).
struct TsParts {
    year: i64,
    month: u32,
    day: u32,
    time: Option<TsTime>,
    // time = (hour, minute, second, fraction_digits, tz_sign_hour_minute, tz_z)
}

type TsTime = (u32, u32, u32, String, Option<(i64, u32, u32)>, bool);

fn match_ts_constructor(v: &str) -> Option<TsParts> {
    let b = v.as_bytes();
    if !b.is_ascii() {
        return None;
    }
    let d = |i: usize| b.get(i).is_some_and(u8::is_ascii_digit);
    if !(d(0) && d(1) && d(2) && d(3)) {
        return None;
    }
    let year: i64 = v[0..4].parse().ok()?;
    let mut i = 4;
    if b.get(i) != Some(&b'-') || !d(i + 1) {
        return None;
    }
    i += 1;
    let m_start = i;
    i += 1;
    if d(i) {
        i += 1;
    }
    let month: u32 = v[m_start..i].parse().ok()?;
    if b.get(i) != Some(&b'-') || !d(i + 1) {
        return None;
    }
    i += 1;
    let d_start = i;
    i += 1;
    if d(i) {
        i += 1;
    }
    let day: u32 = v[d_start..i].parse().ok()?;
    if i == b.len() {
        return Some(TsParts {
            year,
            month,
            day,
            time: None,
        });
    }
    match b.get(i) {
        Some(b'T') | Some(b't') => i += 1,
        Some(b' ') | Some(b'\t') => {
            while matches!(b.get(i), Some(b' ') | Some(b'\t')) {
                i += 1;
            }
        }
        _ => return None,
    }
    if !d(i) {
        return None;
    }
    let h_start = i;
    i += 1;
    if d(i) {
        i += 1;
    }
    let hour: u32 = v[h_start..i].parse().ok()?;
    if b.get(i) != Some(&b':') || !(d(i + 1) && d(i + 2)) {
        return None;
    }
    let minute: u32 = v[i + 1..i + 3].parse().ok()?;
    i += 3;
    if b.get(i) != Some(&b':') || !(d(i + 1) && d(i + 2)) {
        return None;
    }
    let second: u32 = v[i + 1..i + 3].parse().ok()?;
    i += 3;
    let mut fraction = String::new();
    if b.get(i) == Some(&b'.') {
        i += 1;
        while d(i) {
            fraction.push(b[i] as char);
            i += 1;
        }
    }
    let mut tz: Option<(i64, u32, u32)> = None;
    let mut tz_z = false;
    if i < b.len() {
        while matches!(b.get(i), Some(b' ') | Some(b'\t')) {
            i += 1;
        }
        match b.get(i) {
            Some(b'Z') => {
                tz_z = true;
                i += 1;
            }
            Some(&s @ (b'-' | b'+')) => {
                i += 1;
                if !d(i) {
                    return None;
                }
                let th_start = i;
                i += 1;
                if d(i) {
                    i += 1;
                }
                let tz_hour: u32 = v[th_start..i].parse().ok()?;
                let mut tz_minute = 0u32;
                if b.get(i) == Some(&b':') {
                    if !(d(i + 1) && d(i + 2)) {
                        return None;
                    }
                    tz_minute = v[i + 1..i + 3].parse().ok()?;
                    i += 3;
                }
                tz = Some((if s == b'-' { -1 } else { 1 }, tz_hour, tz_minute));
            }
            _ => return None,
        }
        if i != b.len() {
            return None;
        }
    }
    Some(TsParts {
        year,
        month,
        day,
        time: Some((hour, minute, second, fraction, tz, tz_z)),
    })
}

fn construct_yaml_timestamp(node: &Node) -> Result<Yaml, YErr> {
    let value = match node {
        Node::Scalar { value, .. } => value.clone(),
        // Python calls construct_scalar first (errors on non-scalar); if that
        // somehow succeeds (value-key map), the regexp match on `node.value`
        // then crashes the oracle with a TypeError.
        // Reachable only via a value-key mapping (`!!timestamp {=: ...}`):
        // `construct_scalar` succeeds, then `re.match` runs on `node.value`
        // — a list for a MappingNode (verified against Python 3.11.15).
        _ => {
            construct_scalar_value(node)?;
            return Err(YErr::Internal(
                "TypeError: expected string or bytes-like object, got 'list'".to_string(),
            ));
        }
    };
    let parts = match match_ts_constructor(&value) {
        Some(p) => p,
        // Oracle: AttributeError on match.groupdict() (uncaught crash).
        None => {
            return Err(YErr::Internal(
                "AttributeError: 'NoneType' object has no attribute 'groupdict'".to_string(),
            ))
        }
    };
    // datetime.date / datetime.datetime range validation (ValueError crashes
    // in the oracle — PORT-CONTRACT decision 3).
    if parts.year < 1 {
        return Err(YErr::Internal(format!(
            "ValueError: year {} is out of range",
            parts.year
        )));
    }
    if !(1..=12).contains(&parts.month) {
        return Err(YErr::Internal(
            "ValueError: month must be in 1..12".to_string(),
        ));
    }
    if parts.day < 1 || parts.day > days_in_month(parts.year, parts.month) {
        return Err(YErr::Internal(
            "ValueError: day is out of range for month".to_string(),
        ));
    }
    let Some((hour, minute, second, fraction, tz, tz_z)) = parts.time else {
        return Ok(Yaml::Date {
            year: parts.year,
            month: parts.month,
            day: parts.day,
        });
    };
    if hour > 23 {
        return Err(YErr::Internal(
            "ValueError: hour must be in 0..23".to_string(),
        ));
    }
    if minute > 59 {
        return Err(YErr::Internal(
            "ValueError: minute must be in 0..59".to_string(),
        ));
    }
    if second > 59 {
        return Err(YErr::Internal(
            "ValueError: second must be in 0..59".to_string(),
        ));
    }
    let micro = if fraction.is_empty() {
        0
    } else {
        let mut f: String = fraction.chars().take(6).collect();
        while f.len() < 6 {
            f.push('0');
        }
        f.parse::<u32>().unwrap_or(0)
    };
    let tzinfo: Option<i64> = if let Some((sign, th, tm)) = tz {
        let offset = sign * (th as i64 * 3600 + tm as i64 * 60);
        if offset.abs() >= 24 * 3600 {
            return Err(YErr::Internal(format!(
                "ValueError: offset must be a timedelta strictly between -timedelta(hours=24) and timedelta(hours=24), not {}.",
                py_repr_timedelta(offset)
            )));
        }
        Some(offset)
    } else if tz_z {
        Some(0)
    } else {
        None
    };
    Ok(Yaml::DateTime {
        year: parts.year,
        month: parts.month,
        day: parts.day,
        hour,
        minute,
        second,
        micro,
        tz: tzinfo,
    })
}

/// `SafeConstructor.flatten_mapping` — merge-key flattening (reachable only
/// via `!!set` here, since strict-map key construction rejects the merge tag
/// before flattening ever runs on the default map tag).
fn flatten_pairs(pairs: &[(Node, Node)]) -> Result<Vec<(Node, Node)>, YErr> {
    let mut merge: Vec<(Node, Node)> = Vec::new();
    let mut rest: Vec<(Node, Node)> = Vec::new();
    for (k, v) in pairs {
        if k.tag() == TAG_MERGE {
            match v {
                Node::Map {
                    pairs: sub_pairs, ..
                } => {
                    merge.extend(flatten_pairs(sub_pairs)?);
                }
                Node::Seq { items, .. } => {
                    let mut submerge = Vec::new();
                    for sub in items {
                        let Node::Map {
                            pairs: sub_pairs, ..
                        } = sub
                        else {
                            return Err(YErr::Marked(format!(
                                "expected a mapping for merging, but found {}",
                                sub.id()
                            )));
                        };
                        submerge.push(flatten_pairs(sub_pairs)?);
                    }
                    submerge.reverse();
                    for value in submerge {
                        merge.extend(value);
                    }
                }
                _ => {
                    return Err(YErr::Marked(format!(
                        "expected a mapping or list of mappings for merging, but found {}",
                        v.id()
                    )));
                }
            }
        } else if k.tag() == TAG_VALUE {
            let retagged = match k {
                Node::Scalar { value, .. } => Node::Scalar {
                    tag: TAG_STR.to_string(),
                    value: value.clone(),
                },
                other => other.clone(),
            };
            rest.push((retagged, v.clone()));
        } else {
            rest.push((k.clone(), v.clone()));
        }
    }
    merge.extend(rest);
    Ok(merge)
}

/// `BaseConstructor.construct_mapping` after flattening — dict semantics
/// (first-position/last-value on Python-equal keys), used by `!!set`.
fn construct_mapping_plain(node: &Node) -> Result<Vec<(Yaml, Yaml)>, YErr> {
    let Node::Map { pairs, .. } = node else {
        return Err(YErr::Marked(format!(
            "expected a mapping node, but found {}",
            node.id()
        )));
    };
    let flat = flatten_pairs(pairs)?;
    let mut out: Vec<(Yaml, Yaml)> = Vec::new();
    for (k_node, v_node) in &flat {
        let key = construct_value(k_node)?;
        if let Some(_name) = unhashable_type_name(&key) {
            return Err(YErr::Marked("found unhashable key".to_string()));
        }
        let value = construct_value(v_node)?;
        if let Some(slot) = out.iter_mut().find(|(k, _)| py_eq(k, &key)) {
            slot.1 = value;
        } else {
            out.push((key, value));
        }
    }
    Ok(out)
}

/// `_no_duplicates` + `SafeConstructor.construct_mapping` for the default
/// map tag: every key is constructed eagerly and checked against a set with
/// Python equality semantics before any value is constructed.
fn construct_strict_map(node: &Node) -> Result<Yaml, YErr> {
    let Node::Map { pairs, .. } = node else {
        // The oracle's `_no_duplicates` iterates `node.value` directly with
        // no mapping type check, so an explicit `!!map` on a non-mapping node
        // crashes it before the base constructor's ConstructorError can fire.
        // ORACLE DIVERGENCE (PORT-CONTRACT decision 3):
        // mirror those crashes as internal markers, same as every other
        // oracle-crash class. Only *empty* scalars/sequences skip the loop
        // and reach the caught "expected a mapping node" ConstructorError.
        return match node {
            Node::Scalar { value, .. } if !value.is_empty() => {
                // `for key, _ in "<str>"` unpacks 1-char strings.
                Err(YErr::Internal(
                    "ValueError: not enough values to unpack (expected 2, got 1)".to_string(),
                ))
            }
            Node::Seq { items, .. } if !items.is_empty() => Err(YErr::Internal(format!(
                "TypeError: cannot unpack non-iterable {} object",
                py_node_class(&items[0])
            ))),
            _ => Err(YErr::Marked(format!(
                "expected a mapping node, but found {}",
                node.id()
            ))),
        };
    };
    let mut seen: Vec<Yaml> = Vec::new();
    for (k_node, _) in pairs {
        let key = construct_value(k_node)?;
        if unhashable_type_name(&key).is_some() {
            // The healed oracle reports an unhashable key as a structured
            // envelope failure (frontmatter.py `_no_duplicates`) instead of
            // crashing at the `key in seen` test, so the former decision-3
            // divergence marker converges to the same malformed-frontmatter
            // issue text. py_repr mirrors the oracle's `{key!r}`.
            return Err(YErr::Marked(format!(
                "unhashable frontmatter key: {}",
                py_repr(&key)?
            )));
        }
        if seen.iter().any(|s| py_eq(s, &key)) {
            // py_repr can itself crash the oracle (4300-digit str limit).
            return Err(YErr::Marked(format!(
                "duplicate frontmatter key: {}",
                py_repr(&key)?
            )));
        }
        seen.push(key);
    }
    // flatten_mapping is a no-op here: merge/value-tagged keys already failed
    // during eager key construction above.
    let mut out: Vec<(Yaml, Yaml)> = Vec::new();
    for (k_node, v_node) in pairs {
        let key = construct_value(k_node)?;
        let value = construct_value(v_node)?;
        out.push((key, value));
    }
    Ok(Yaml::Map(out))
}

fn construct_omap_pairs(node: &Node) -> Result<Yaml, YErr> {
    // Context strings differ ("an ordered map" vs "pairs") but only the
    // problem text reaches output, and those are identical.
    let Node::Seq { items, .. } = node else {
        return Err(YErr::Marked(format!(
            "expected a sequence, but found {}",
            node.id()
        )));
    };
    let mut out: Vec<Yaml> = Vec::new();
    for sub in items {
        let Node::Map { pairs, .. } = sub else {
            return Err(YErr::Marked(format!(
                "expected a mapping of length 1, but found {}",
                sub.id()
            )));
        };
        if pairs.len() != 1 {
            return Err(YErr::Marked(format!(
                "expected a single mapping item, but found {} items",
                pairs.len()
            )));
        }
        let key = construct_value(&pairs[0].0)?;
        let value = construct_value(&pairs[0].1)?;
        out.push(Yaml::Tuple(vec![key, value]));
    }
    Ok(Yaml::List(out))
}

fn construct_value(node: &Node) -> Result<Yaml, YErr> {
    match node.tag() {
        TAG_MAP => construct_strict_map(node),
        TAG_STR => Ok(Yaml::Str(construct_scalar_value(node)?)),
        TAG_NULL => {
            construct_scalar_value(node)?;
            Ok(Yaml::Null)
        }
        TAG_BOOL => construct_yaml_bool(node),
        TAG_INT => construct_yaml_int(node),
        TAG_FLOAT => construct_yaml_float(node),
        "tag:yaml.org,2002:binary" => construct_yaml_binary(node),
        TAG_TIMESTAMP => construct_yaml_timestamp(node),
        TAG_SEQ => {
            let Node::Seq { items, .. } = node else {
                return Err(YErr::Marked(format!(
                    "expected a sequence node, but found {}",
                    node.id()
                )));
            };
            let mut out = Vec::new();
            for item in items {
                out.push(construct_value(item)?);
            }
            Ok(Yaml::List(out))
        }
        "tag:yaml.org,2002:omap" | "tag:yaml.org,2002:pairs" => construct_omap_pairs(node),
        "tag:yaml.org,2002:set" => {
            let pairs = construct_mapping_plain(node)?;
            Ok(Yaml::Set(pairs.into_iter().map(|(k, _)| k).collect()))
        }
        other => Err(YErr::Marked(format!(
            "could not determine a constructor for the tag {}",
            py_repr_str(other)
        ))),
    }
}

/// `yaml.load(raw, Loader=_BoundedLoader)`.
fn yaml_load(raw: &str) -> Result<Yaml, YErr> {
    check_printable(raw)?;
    let scanner = Scanner::new(raw);
    let parser = Parser::new(scanner);
    let mut composer = Composer {
        p: parser,
        anchors: std::collections::HashSet::new(),
        depth: 0,
    };
    match composer.get_single_node()? {
        None => Ok(Yaml::Null),
        Some(node) => construct_value(&node),
    }
}

// ---------------------------------------------------------------------------
// parse_frontmatter — envelope load + field validation
// ---------------------------------------------------------------------------

fn map_get<'a>(pairs: &'a [(Yaml, Yaml)], name: &str) -> Option<&'a Yaml> {
    pairs.iter().find_map(|(k, v)| match k {
        Yaml::Str(s) if s == name => Some(v),
        _ => None,
    })
}

/// The envelope load (`_load_frontmatter_mapping`): oversize gate, bounded
/// YAML load, exception→issue mapping, non-mapping rejection. Public so the
/// conformance vectors can compare the loaded value model directly.
pub fn load_frontmatter_mapping(raw: &str) -> (Option<Vec<(Yaml, Yaml)>>, Vec<Issue>) {
    if exceeds_byte_cap(raw, MAX_FRONTMATTER_BYTES) {
        return (
            None,
            vec![Issue::error(
                "malformed-frontmatter",
                format!("frontmatter exceeds the {MAX_FRONTMATTER_BYTES}-byte cap"),
            )],
        );
    }
    match yaml_load(raw) {
        Ok(Yaml::Map(pairs)) => (Some(pairs), Vec::new()),
        Ok(_) => (
            None,
            vec![Issue::error(
                "malformed-frontmatter",
                "frontmatter must be a YAML mapping of supported fields".to_string(),
            )],
        ),
        Err(YErr::Marked(problem)) => {
            if problem.contains("duplicate frontmatter key") {
                (
                    None,
                    vec![Issue::error("duplicate-frontmatter-key", problem)],
                )
            } else {
                (
                    None,
                    vec![Issue::error(
                        "malformed-frontmatter",
                        format!("frontmatter is not valid YAML: {problem}"),
                    )],
                )
            }
        }
        Err(YErr::Reader(msg)) => (
            None,
            vec![Issue::error(
                "malformed-frontmatter",
                format!("frontmatter is not valid YAML: {msg}"),
            )],
        ),
        // ORACLE DIVERGENCE (PORT-CONTRACT decision 3): these inputs crash
        // the oracle with an uncaught exception; we return a distinguishable
        // internal issue instead. The code below never appears in oracle
        // output, so any occurrence flags the divergence for the harness.
        Err(YErr::Internal(msg)) => (
            None,
            vec![Issue {
                severity: "error",
                code: "internal-oracle-divergence".to_string(),
                message: msg,
                line: None,
            }],
        ),
    }
}

/// Full-document YAML load for the strict `.decided/config.yaml` readers
/// (`decided gate`, `decided.services.init._parse_config_yaml`): `Ok(root)` on a
/// clean parse, `Err(problem text)` otherwise. The oracle embeds PyYAML's
/// exact multi-line exception prose in its `invalid YAML: <exc>` message;
/// that prose is not byte-reproducible here (stderr is out of parity
/// scope), so the error text is the engine's own parse problem.
pub fn yaml_load_config(raw: &str) -> Result<Yaml, String> {
    yaml_load(raw).map_err(|e| match e {
        YErr::Marked(p) | YErr::Reader(p) | YErr::Internal(p) => p,
    })
}

fn check_unknown_fields(pairs: &[(Yaml, Yaml)], issues: &mut Vec<Issue>) -> Result<(), YErr> {
    for (key, _) in pairs {
        let known = matches!(key, Yaml::Str(s) if SUPPORTED_FIELDS.contains(&s.as_str()));
        if !known {
            issues.push(Issue::error(
                "invalid-metadata-field",
                format!(
                    "unsupported frontmatter field: {} (supported: {})",
                    py_repr(key)?,
                    SUPPORTED_FIELDS.join(", ")
                ),
            ));
        }
    }
    Ok(())
}

fn validate_schema_version(
    pairs: &[(Yaml, Yaml)],
    issues: &mut Vec<Issue>,
) -> Result<Option<SchemaVersion>, YErr> {
    let Some(value) = map_get(pairs, "schema_version") else {
        issues.push(Issue::error(
            "invalid-metadata-field",
            "frontmatter is missing required field 'schema_version'".to_string(),
        ));
        return Ok(None); // data.get() is None here
    };
    // Python: isinstance(v, int) and not isinstance(v, bool) — bignums pass.
    let (supported, sv) = match value {
        Yaml::Int(v) => (SUPPORTED_SCHEMA_VERSIONS.contains(v), SchemaVersion::Int(*v)),
        Yaml::BigInt(b) => (false, SchemaVersion::Big(b.clone())),
        _ => {
            issues.push(Issue::error(
                "invalid-metadata-field",
                "frontmatter field 'schema_version' must be an integer".to_string(),
            ));
            return Ok(None);
        }
    };
    if !supported {
        // f-string str(int): over the 4300-digit limit the oracle crashes.
        if let SchemaVersion::Big(b) = &sv {
            if b.digits.len() > INT_MAX_STR_DIGITS {
                return Err(int_to_str_limit_err());
            }
        }
        issues.push(Issue::error(
            "unsupported-schema-version",
            format!(
                "unsupported frontmatter schema_version: {} (supported: {})",
                sv,
                SUPPORTED_SCHEMA_VERSIONS
                    .iter()
                    .map(|s| s.to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        ));
    }
    Ok(Some(sv))
}

fn validate_id(pairs: &[(Yaml, Yaml)], issues: &mut Vec<Issue>) -> Option<String> {
    let value = map_get(pairs, "id")?;
    if matches!(value, Yaml::Null) {
        return None;
    }
    let Yaml::Str(s) = value else {
        issues.push(Issue::error(
            "invalid-metadata-field",
            "frontmatter field 'id' must be a string".to_string(),
        ));
        return None;
    };
    if !is_valid_id(s) {
        issues.push(Issue::error(
            "invalid-id-syntax",
            format!(
                "invalid artifact ID syntax: {} (expected <KEY>-<12-char Crockford base32 suffix>, e.g. RAC-01JY4M8X2QZ7)",
                py_repr_str(s)
            ),
        ));
        return None;
    }
    Some(normalize_id(s))
}

fn validate_type(
    pairs: &[(Yaml, Yaml)],
    issues: &mut Vec<Issue>,
) -> Result<Option<String>, YErr> {
    let Some(value) = map_get(pairs, "type") else {
        return Ok(None);
    };
    if matches!(value, Yaml::Null) {
        return Ok(None);
    }
    let registered = match value {
        Yaml::Str(s) => crate::spec::spec_for(s).is_some(),
        _ => false,
    };
    if !registered {
        issues.push(Issue::error(
            "invalid-metadata-field",
            format!(
                "frontmatter field 'type' is not a registered artifact type: {}",
                py_repr(value)?
            ),
        ));
        return Ok(None);
    }
    Ok(match value {
        Yaml::Str(s) => Some(s.clone()),
        _ => None,
    })
}

fn validate_relationships(
    pairs: &[(Yaml, Yaml)],
    issues: &mut Vec<Issue>,
) -> Vec<(String, Vec<String>)> {
    let Some(value) = map_get(pairs, "relationships") else {
        return Vec::new();
    };
    if matches!(value, Yaml::Null) {
        return Vec::new();
    }
    let well_formed = match value {
        Yaml::Map(rel_pairs) => rel_pairs.iter().all(|(kind, targets)| {
            matches!(kind, Yaml::Str(_))
                && matches!(targets, Yaml::List(items) if items.iter().all(|t| matches!(t, Yaml::Str(_))))
        }),
        _ => false,
    };
    if !well_formed {
        issues.push(Issue::error(
            "invalid-metadata-field",
            "frontmatter field 'relationships' must map relationship kinds to lists of artifact IDs"
                .to_string(),
        ));
        return Vec::new();
    }
    let Yaml::Map(rel_pairs) = value else {
        return Vec::new();
    };
    rel_pairs
        .iter()
        .map(|(kind, targets)| {
            let k = match kind {
                Yaml::Str(s) => s.clone(),
                _ => String::new(),
            };
            let t = match targets {
                Yaml::List(items) => items
                    .iter()
                    .map(|t| match t {
                        Yaml::Str(s) => normalize_id(s),
                        _ => String::new(),
                    })
                    .collect(),
                _ => Vec::new(),
            };
            (k, t)
        })
        .collect()
}

fn parse_tags(pairs: &[(Yaml, Yaml)], issues: &mut Vec<Issue>) -> Vec<String> {
    let Some(value) = map_get(pairs, "tags") else {
        return Vec::new();
    };
    if matches!(value, Yaml::Null) {
        return Vec::new();
    }
    let well_formed = matches!(value, Yaml::List(items) if items
        .iter()
        .all(|t| matches!(t, Yaml::Str(s) if !py_strip(s).is_empty())));
    if !well_formed {
        issues.push(Issue::error(
            "invalid-metadata-field",
            "frontmatter field 'tags' must be a list of non-empty strings".to_string(),
        ));
        return Vec::new();
    }
    let Yaml::List(items) = value else {
        return Vec::new();
    };
    items
        .iter()
        .map(|t| match t {
            Yaml::Str(s) => py_strip(s).to_string(),
            _ => String::new(),
        })
        .collect()
}

/// Parse and schema-validate raw frontmatter YAML.
///
/// Returns `(metadata, issues)`: metadata is `None` only on envelope-level
/// failures (oversize, malformed YAML, alias, depth, duplicate key,
/// non-mapping top level); field-level problems always return a constructed
/// metadata. Issue order is pinned: unknown fields (document order), then
/// schema_version, id, type, relationships, tags.
pub fn parse_frontmatter(raw: &str) -> (Option<ArtifactMetadata>, Vec<Issue>) {
    let (data, mut issues) = load_frontmatter_mapping(raw);
    let Some(pairs) = data else {
        return (None, issues);
    };
    match validate_fields(&pairs, &mut issues) {
        Ok(metadata) => (Some(metadata), issues),
        // ORACLE DIVERGENCE (PORT-CONTRACT decision 3): message formatting in
        // a field validator can crash the oracle (4300-digit int->str limit
        // on a bignum key/value). Mirror it as the internal marker; nothing
        // else survives the oracle's crash, so earlier issues are dropped.
        Err(YErr::Internal(msg)) => (
            None,
            vec![Issue {
                severity: "error",
                code: "internal-oracle-divergence".to_string(),
                message: msg,
                line: None,
            }],
        ),
        // Validators only raise the Internal class.
        Err(_) => unreachable!("field validators raise only internal errors"),
    }
}

fn validate_fields(
    pairs: &[(Yaml, Yaml)],
    issues: &mut Vec<Issue>,
) -> Result<ArtifactMetadata, YErr> {
    check_unknown_fields(pairs, issues)?;
    let schema_version = validate_schema_version(pairs, issues)?;
    let id = validate_id(pairs, issues);
    let artifact_type = validate_type(pairs, issues)?;
    let relationships = validate_relationships(pairs, issues);
    let tags = parse_tags(pairs, issues);
    Ok(ArtifactMetadata {
        schema_version: schema_version.unwrap_or(SchemaVersion::Int(0)),
        id,
        artifact_type,
        relationships,
        tags,
        provenance: "frontmatter",
    })
}

// ---------------------------------------------------------------------------
// parse_file support (src/rac/core/markdown.py read stage) — the frontmatter
// contract owns the wordings; the markdown module sequences the issues.
// ---------------------------------------------------------------------------

/// "file cap" wording — emitted by `parse_file` for an oversized file.
pub fn oversize_file_issue(cap: u64) -> Issue {
    Issue {
        severity: "error",
        code: "artifact-oversize".to_string(),
        message: format!("artifact exceeds the {cap}-byte file cap (set DECIDED_MAX_FILE_BYTES to raise it)"),
        line: Some(1),
    }
}

/// "parse cap" wording — emitted by `parse` for oversized text. Pinned as
/// distinct from the file-cap wording; do not unify.
pub fn oversize_parse_issue(cap: u64) -> Issue {
    Issue {
        severity: "error",
        code: "artifact-oversize".to_string(),
        message: format!("artifact exceeds the {cap}-byte parse cap (set DECIDED_MAX_FILE_BYTES to raise it)"),
        line: Some(1),
    }
}

pub fn non_utf8_issue() -> Issue {
    Issue {
        severity: "warning",
        code: "non-utf8-content".to_string(),
        message: "artifact is not valid UTF-8; decoded lossily".to_string(),
        line: Some(1),
    }
}

/// The unterminated-frontmatter issue `markdown.parse` appends when
/// `split.raw is None and split.unterminated`.
pub fn unterminated_issue() -> Issue {
    Issue {
        severity: "error",
        code: "malformed-frontmatter".to_string(),
        message: "frontmatter block opened with --- on line 1 but never closed".to_string(),
        line: Some(1),
    }
}

#[derive(Debug)]
pub struct ArtifactRead {
    /// Decoded text (lossily when `lossy`); None on oversize/unreadable.
    pub text: Option<String>,
    /// The terminal read issue (oversize / unreadable), if any.
    pub issue: Option<Issue>,
    /// True when the bytes were not valid UTF-8 (warning appended by the
    /// caller AFTER parsing, as the last parse issue).
    pub lossy: bool,
}

/// Python `str(OSError)`: `[Errno N] <strerror>: '<path>'`.
fn py_oserror_message(e: &std::io::Error, path: &str) -> String {
    let s = e.to_string();
    let msg = match s.find(" (os error") {
        Some(pos) => &s[..pos],
        None => s.as_str(),
    };
    match e.raw_os_error() {
        Some(n) => format!("[Errno {n}] {msg}: '{path}'"),
        None => format!("{msg}: '{path}'"),
    }
}

/// The read stage of `parse_file`: size check, capped read, strict-then-lossy
/// UTF-8 decode (`errors="replace"`, one U+FFFD per bogus byte — Rust's
/// `from_utf8_lossy` follows the same WHATWG policy).
pub fn read_artifact_text(path: &str) -> ArtifactRead {
    use std::io::Read;
    let cap_state = file_cap();
    let unreadable = |e: &std::io::Error| ArtifactRead {
        text: None,
        issue: Some(Issue {
            severity: "error",
            code: "unreadable-artifact".to_string(),
            message: format!("cannot read artifact: {}", py_oserror_message(e, path)),
            line: Some(1),
        }),
        lossy: false,
    };
    let size = match std::fs::metadata(path) {
        Ok(m) => m.len(),
        Err(e) => return unreadable(&e),
    };
    let cap = match cap_state {
        FileCap::Cap(cap) => cap,
        // ORACLE DIVERGENCE (PORT-CONTRACT decision 3): a cap >= 2^63 - 1
        // makes the oracle's `fh.read(cap + 1)` crash uncaught on EVERY
        // successfully opened file. Mirror it as the marker. The oracle stats the path and opens the file first,
        // so an unreadable path still reports unreadable-artifact.
        FileCap::OracleCrash(msg) => {
            return match std::fs::File::open(path) {
                Ok(_) => ArtifactRead {
                    text: None,
                    issue: Some(Issue {
                        severity: "error",
                        code: "internal-oracle-divergence".to_string(),
                        message: msg.to_string(),
                        line: None,
                    }),
                    lossy: false,
                },
                Err(e) => unreadable(&e),
            };
        }
    };
    if size > cap {
        return ArtifactRead {
            text: None,
            issue: Some(oversize_file_issue(cap)),
            lossy: false,
        };
    }
    let mut data = Vec::new();
    match std::fs::File::open(path) {
        Ok(f) => {
            let mut handle = f.take(cap + 1);
            if let Err(e) = handle.read_to_end(&mut data) {
                return unreadable(&e);
            }
        }
        Err(e) => return unreadable(&e),
    }
    if data.len() as u64 > cap {
        return ArtifactRead {
            text: None,
            issue: Some(oversize_file_issue(cap)),
            lossy: false,
        };
    }
    match String::from_utf8(data) {
        Ok(text) => ArtifactRead {
            text: Some(text),
            issue: None,
            lossy: false,
        },
        Err(e) => {
            let text = String::from_utf8_lossy(e.as_bytes()).into_owned();
            ArtifactRead {
                text: Some(text),
                issue: None,
                lossy: true,
            }
        }
    }
}
