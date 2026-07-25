# 02 — Frontmatter contract (`src/rac/core/frontmatter.py`)

Status: verified against the oracle venv (`.venv-oracle`, Python 3.11, PyYAML 6.0.3)
on 2026-07-11 unless a claim is marked UNVERIFIED. This is parity landmine #1: the
module is a *bounded PyYAML SafeLoader* (full YAML 1.1 grammar) plus three guards —
not a hand-rolled mini-grammar. Byte-for-byte parity requires reproducing PyYAML
1.1 implicit resolution, PyYAML error *problem strings*, and Python `repr()`
formatting inside issue messages.

Public surface consumed downstream:

- `split_frontmatter(text: str) -> FrontmatterSplit`
- `parse_frontmatter(raw: str) -> (ArtifactMetadata | None, list[Issue])`
- Constants used: `MAX_FRONTMATTER_BYTES = 65536` (64 KiB), `MAX_FRONTMATTER_DEPTH = 32`
  (both in `src/rac/core/limits.py`).
- `Issue` (in `models.py`): `(severity, code, message, line=None)`. Every issue this
  module emits has `severity="error"` and `line=None`. (The one frontmatter-related
  issue with a line number — unterminated block — is emitted by `markdown.parse`,
  see §8.)
- `ArtifactMetadata` (in `metadata.py`): `schema_version: int`, `id: str|None`,
  `type: str|None`, `relationships: dict[str, list[str]]`, `tags: list[str]`,
  `provenance: str = "frontmatter"` (always the default here).
- `SUPPORTED_SCHEMA_VERSIONS = (1,)`.

---

## 1. `split_frontmatter(text)` — delimiter rules

Algorithm (exact):

1. `lines = text.split("\n")` — split on LF **only**. CR is never a line break here;
   CRLF files leave a trailing `"\r"` on every line.
2. If `lines[0].strip() != "---"` → `FrontmatterSplit(raw=None, body=text, line_offset=0, unterminated=False)`.
   (`text.split("\n")` always yields ≥1 element, even for `""`.)
3. Otherwise scan `i = 1 .. len(lines)-1` for the **first** line whose `.strip()` is
   `"---"` **or** `"..."` (the two closers). On hit:
   - `raw = "\n".join(lines[1:i])` — the text strictly between the delimiter lines,
     LF-joined, **no trailing newline**, `\r` bytes retained on CRLF input.
   - `body = "\n".join(lines[i+1:])` — may be `""`.
   - `line_offset = i + 1` (body line N, 1-based, is file line `N + line_offset`).
   - `unterminated = False`.
4. No closer found → `FrontmatterSplit(raw=None, body=text, line_offset=0, unterminated=True)`.

`str.strip()` semantics (critical): strips Python *Unicode whitespace* from both
ends — space, `\t`, `\n`, `\r`, `\x0b`, `\x0c`, `\x1c`–`\x1f`, `\x85` (NEL),
`\xa0` (NBSP), ` `–` `, ` `, ` `, `　`, etc. — i.e.
characters where `str.isspace()` is true. Consequences (all verified):

- `" ---"`, `"\t---\t"`, `"\xa0---\xa0"`, `"\x0b---"` as line 1 **all open** frontmatter.
- `"---   "` (trailing spaces) opens; `"   ...   "` closes.
- **U+FEFF (BOM) is NOT whitespace**: a file beginning `﻿---` has **no
  frontmatter** — the whole text is body, no issue is raised, the `---` line is
  ordinary Markdown. (BOM-defeats-frontmatter; verified end-to-end via `parse_file`
  on a `EF BB BF`-prefixed file: `metadata=None`, `metadata_issues=[]`.)
- U+200B (zero-width space) is not whitespace either → defeats the delimiter. UNVERIFIED.
- `"----"` or `"--- yaml"` on line 1 → not a delimiter → no frontmatter.
- `"\n---\n..."` (blank first line) → no frontmatter. Only line 1 counts.
- CRLF: `"---\r"` strips to `"---"` → opens/closes fine; `raw` keeps interior `\r`
  (PyYAML treats CR as a line break, so values are unaffected).

Verified split examples:

| input | raw | body | offset | unterminated |
|---|---|---|---|---|
| `"---\na: 1\n---\nbody"` | `'a: 1'` | `'body'` | 3 | False |
| `"---\na: 1\n...\nbody"` | `'a: 1'` | `'body'` | 3 | False |
| `"---\n---"` | `''` | `''` | 2 | False |
| `"---"` | None | `'---'` | 0 | **True** |
| `"---\na: 1\nbody"` | None | whole text | 0 | **True** |
| `""` | None | `''` | 0 | False |
| `"﻿---\na: 1\n---\nbody"` | None | whole text | 0 | False |
| `"---\r\na: 1\r\n---\r\nbody\r\n"` | `'a: 1\r'` | `'body\r\n'` | 3 | False |

---

## 2. `parse_frontmatter(raw)` — pipeline and return contract

1. **Envelope load** (`_load_frontmatter_mapping`, §3). On any envelope failure
   returns `(None, [single terminal Issue])` — metadata is `None` **only** on these
   paths: oversize raw, YAML error, alias, depth, duplicate key, non-mapping top level.
2. **Field validation** on the loaded `dict`. Metadata is **always** constructed
   (never None) once the mapping loads, no matter how many field issues there are.
   Issue order is pinned: unknown fields first (in YAML document order), then
   `schema_version`, `id`, `type`, `relationships`, `tags`. At most one issue per
   field; max total = (#unknown keys) + 5.

Construction: `schema_version = validated value if isinstance(int) else 0` (note:
an *unsupported* int like `2` is stored as-is → `schema_version=2`; missing/non-int
→ `0`), `provenance="frontmatter"` always.

---

## 3. Envelope load — bounds, loader, error mapping

Order of checks:

**(a) Byte cap, before any YAML work.** `exceeds_byte_cap(raw, 65536)` is exactly
"UTF-8 byte length of `raw` > 65536" (the char-count shortcuts in `limits.py` are a
pure optimization: `len(raw) > cap` → True; `len(raw) <= cap//4` → False; else
encode and compare). Boundary verified: 65536 bytes OK, 65537 rejected. The cap is
measured on the **raw block only** (delimiter lines and the newline before the
closer excluded). Failure:

```
error / malformed-frontmatter / "frontmatter exceeds the 65536-byte cap"
```

(The number is the f-stringed constant; if the constant ever changed the message
changes with it.)

**(b) `yaml.load(raw, Loader=_BoundedLoader)`** — PyYAML `SafeLoader` subclass with:

- **Duplicate-key rejection** (all mappings, any depth, incl. explicit `!!map`):
  before constructing a mapping, each key node is constructed (`deep=True`) and
  checked against a Python `set`. First repeat raises with problem
  `f"duplicate frontmatter key: {key!r}"`. Caught specially (substring match
  `"duplicate frontmatter key" in exc.problem`) and reported as:
  ```
  error / duplicate-frontmatter-key / duplicate frontmatter key: <repr(key)>
  ```
  **Python-equality semantics** (verified):
  - `a: 1` + `a: 2` → `duplicate frontmatter key: 'a'` (Python str repr — single
    quotes; switches to double quotes when the string contains `'` and no `"`:
    `"it's"` → `duplicate frontmatter key: "it's"`).
  - Quoted vs plain same text (`'a':` vs `a:`) → duplicate.
  - **`1:` + `true:` → duplicate** (Python `1 == True`), message
    `duplicate frontmatter key: True` (repr of the *second* occurrence's key).
  - **`1:` + `1.0:` → duplicate** (`1 == 1.0`), message `duplicate frontmatter key: 1.0`.
  - `yes:` + `on:` → duplicate (`True`), message `duplicate frontmatter key: True`.
  - `a:` + `A:` → NOT duplicate (case-sensitive).
  - `.nan:` twice → **duplicate detected** (observed; PyYAML yields the same/equal
    NaN object so set membership hits), message `duplicate frontmatter key: nan`.
  - **Unhashable key crash**: a collection key (`? [1, 2]\n: x`) raises an uncaught
    `TypeError: unhashable type: 'list'` out of `parse_frontmatter` — the `key in
    seen` test runs before PyYAML's own "found unhashable key" guard. Verified.
    This propagates to callers (would crash `parse()`); the Rust port must decide
    to replicate the crash or the caller-level effect — flag for the parity harness.
- **Alias rejection** (in `compose_node`): any `*alias` event raises problem
  `"YAML aliases are not permitted in frontmatter"` →
  ```
  error / malformed-frontmatter / frontmatter is not valid YAML: YAML aliases are not permitted in frontmatter
  ```
  **Anchors alone are permitted**: `a: &x 1\nb: 2` parses fine (verified). Only
  *use* of an alias is rejected.
- **Depth cap** (in `compose_node`): a counter increments per node being composed
  (root mapping = depth 1; every mapping/sequence/**scalar** counts one level).
  If depth would exceed 32:
  ```
  error / malformed-frontmatter / frontmatter is not valid YAML: frontmatter nesting exceeds the 32-level cap
  ```
  Verified geometry: `a: ` + 30 nested flow seqs (`[[[...1...]]]`) passes — root
  map d1, seqs d2–d31, innermost scalar d32. 31 seqs fails. Block-map chains: 30
  nested mappings under the root pass, 31 fail. The counter is decremented on the
  way out (`finally`), so a wide-but-shallow document never trips it.

**(c) Exception → issue mapping** (order of `except` clauses matters):

| raised | issue |
|---|---|
| `MarkedYAMLError` with `"duplicate frontmatter key"` in `problem` | `duplicate-frontmatter-key`, message = `exc.problem` verbatim |
| any other `MarkedYAMLError` (Scanner/Parser/Composer/Constructor errors, the alias & depth raises) | `malformed-frontmatter`, message = `f"frontmatter is not valid YAML: {exc.problem}"` — **only the `problem` field**, no marks/context |
| other `yaml.YAMLError` (in practice `ReaderError` for forbidden control characters) | `malformed-frontmatter`, message = `f"frontmatter is not valid YAML: {exc}"` — **full multi-line str** |
| `RecursionError` | `malformed-frontmatter`, `"frontmatter nesting too deep to parse"` — in practice unreachable (depth cap pre-empts). UNVERIFIED (could not trigger) |

Verified `problem` strings the port must reproduce (these are PyYAML 6.0.3
message literals — full parity means porting PyYAML's scanner/parser message
catalog):

- unclosed flow: `frontmatter is not valid YAML: expected ',' or ']', but got '<stream end>'`
  (and `'}'` variant for `{a: 1`)
- tab indent (`a:\n\tb: 1`): `frontmatter is not valid YAML: found character '\t' that cannot start any token`
- `a: b:\n c`: problem `mapping values are not allowed here`
- leading `%YAML 1.2` directive: `frontmatter is not valid YAML: expected '<document start>', but found '<scalar>'`
- unknown/unsupported tag (`!!python/object {}`): `frontmatter is not valid YAML: could not determine a constructor for the tag 'tag:yaml.org,2002:python/object'`
- **merge key `<<:`**: `... could not determine a constructor for the tag 'tag:yaml.org,2002:merge'` — YAML 1.1 merge is resolved but SafeConstructor has no constructor, and the dup-check constructs key nodes eagerly, so **merge keys always fail** (verified)
- **`=` key**: `... could not determine a constructor for the tag 'tag:yaml.org,2002:value'` (verified)
- NUL byte (`a: \x00`): ReaderError path, message is **multi-line**:
  `frontmatter is not valid YAML: unacceptable character #x0000: special characters are not allowed\n  in "<unicode string>", position 3`

**(d) Non-mapping top level** (successful load of a scalar/sequence/`None`):

```
error / malformed-frontmatter / frontmatter must be a YAML mapping of supported fields
```

Triggers: empty raw (`---\n---`), comments-only raw, `- a\n- b`, bare scalar. Verified.

---

## 4. The accepted YAML grammar — full YAML 1.1 via SafeLoader

Everything PyYAML's SafeLoader accepts is accepted (block & flow mappings and
sequences, all scalar styles — plain, single/double-quoted, literal `|`, folded
`>` — comments, multi-word keys, explicit `?` keys, anchors) **except**: aliases,
depth > 32, duplicate keys, tags without a Safe constructor (which includes `<<`
merge and `=` value keys). Multiple documents inside `raw` are impossible because
`split_frontmatter` terminates the block at the first full-line `---`/`...`;
`%` directives fail as shown above.

Explicit tags that DO construct (SafeConstructor set): `!!str`, `!!int`, `!!float`,
`!!bool`, `!!null`, `!!timestamp`, `!!binary` (→ `bytes`), `!!seq`, `!!map`,
`!!set`, `!!omap`, `!!pairs`. E.g. `a: !!str 2026-07-11` yields the *string*
(verified). `!!binary` values will then fail field type checks. Any other tag →
ConstructorError → malformed.

### Implicit scalar resolution — exact PyYAML 6.0.3 (YAML 1.1) resolver regexes

Applied to **plain** scalars only (quoted scalars are always strings). Quoted
verbatim from the installed `yaml/resolver.py` (all `re.X` — whitespace in the
pattern is insignificant); a tag is tried only if the scalar's first character is
in its trigger set, and resolution order for a matching first char follows
registration below. Anything matching nothing is `!!str`.

**bool** (first chars `yYnNtTfFoO`):
```
^(?:yes|Yes|YES|no|No|NO|true|True|TRUE|false|False|FALSE|on|On|ON|off|Off|OFF)$
```
Note: single `y`/`n` are **NOT** booleans (they resolve to strings) even though
YAML 1.1 spec lists them — PyYAML's regex omits them. Verified: `e: y` → `'y'`.

**float** (first chars `-+0123456789.`):
```
^(?: [-+]?(?:[0-9][0-9_]*)\.[0-9_]*(?:[eE][-+][0-9]+)?
   | \.[0-9][0-9_]*(?:[eE][-+][0-9]+)?
   | [-+]?[0-9][0-9_]*(?::[0-5]?[0-9])+\.[0-9_]*
   | [-+]?\.(?:inf|Inf|INF)
   | \.(?:nan|NaN|NAN) )$
```
Landmines (verified): a dot is mandatory for ordinary floats and the exponent
**sign is mandatory** — `1e5` AND `1e+5` are both **strings** (no dot); `6.` →
`6.0`; `1_0.5` → `10.5` (underscores); `1:30.0` → sexagesimal `90.0`; `.inf`/
`-.Inf` → ±inf; `.nan` → NaN. `inf`/`nan` without the dot are strings.

**int** (first chars `-+0123456789`):
```
^(?: [-+]?0b[0-1_]+
   | [-+]?0[0-7_]+
   | [-+]?(?:0|[1-9][0-9_]*)
   | [-+]?0x[0-9a-fA-F_]+
   | [-+]?[1-9][0-9_]*(?::[0-5]?[0-9])+ )$
```
Verified: `0x1F` → 31, `010` → 8 (octal!), `0b101` → 5, `1_000` → 1000,
`1:30` → **90** (sexagesimal), `+5` → 5, `-0` → 0.

**merge** `^(?:<<)$` (first char `<`) and **value** `^(?:=)$` (first char `=`):
resolved, then construction fails (§3c).

**null** (first chars `~`, `n`, `N`, `''` i.e. empty):
```
^(?:~|null|Null|NULL|)$
```
Verified: `~`, `null`, `Null`, `NULL`, and the **empty scalar** (`e:`) are all
`None`; `none` is a string.

**timestamp** (first chars digits):
```
^(?: [0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]
   | [0-9][0-9][0-9][0-9]-[0-9][0-9]?-[0-9][0-9]?
     (?:[Tt]|[ \t]+)[0-9][0-9]?:[0-9][0-9]:[0-9][0-9](?:\.[0-9]*)?
     (?:[ \t]*(?:Z|[-+][0-9][0-9]?(?::[0-9][0-9])?))?)$
```
Verified: `2026-07-11` → `datetime.date(2026, 7, 11)`; `2026-07-11T10:00:00Z` →
tz-aware `datetime`; `2026-7-1` (1-digit fields, date-only form) → **string**
(date-only form requires 2-digit month/day). These non-string values then fail
field validators with the messages in §5 (`repr()` shown, e.g.
`datetime.date(2026, 7, 11)`).

Keys resolve identically — `1: x` has int key `1`, `true:`/`yes:`/`on:` key
`True`, `2026-07-11:` a `date` key, `null:` key `None`. `dict` preserves document
order, which fixes unknown-field issue order.

---

## 5. Field validators — exact messages and coercions

All are `severity="error"`, `line=None`. `{...!r}` denotes Python `repr()` output
(str repr rules: prefer `'...'`; use `"..."` iff the string contains `'` and no
`"`; escape `\\`, `\n`, `\t`, non-printables as `\xNN`/`\uNNNN`; ints/bools/None
as `1`/`True`/`None`; floats via Python float repr; dates as
`datetime.date(Y, M, D)`).

**Unknown fields** — for every key not in
`("schema_version", "id", "type", "relationships", "tags")`, in document order:

```
invalid-metadata-field / unsupported frontmatter field: {key!r} (supported: schema_version, id, type, relationships, tags)
```

Verified reprs: `'zzz'`, `2` (int key), `None` (null key), `datetime.date(2026, 7, 11)`,
`"we're"`.

**schema_version** (required):
- absent → `invalid-metadata-field / frontmatter is missing required field 'schema_version'`;
  stored `schema_version=0`.
- present but not an int, **or a bool** (`yes`/`true`), or null, float, string →
  `invalid-metadata-field / frontmatter field 'schema_version' must be an integer`;
  stored `0`. (`"1"` quoted → error; `1.0` → error.)
- int but ∉ `(1,)` →
  `unsupported-schema-version / unsupported frontmatter schema_version: {v} (supported: 1)`;
  stored **as-is** (e.g. `2`, `90`, `10`).
- Any YAML-1.1 int spelling of 1 is valid: `0x1`, `01`, `+1` all → `1`, no issue
  (verified). `1:30` → 90 → unsupported. `1_0` → 10 → unsupported.

**id** (optional; `id: null` == absent, no issue):
- non-string → `invalid-metadata-field / frontmatter field 'id' must be a string`; stored None.
- string failing syntax →
  `invalid-id-syntax / invalid artifact ID syntax: {original!r} (expected <KEY>-<12-char Crockford base32 suffix>, e.g. RAC-01JY4M8X2QZ7)`;
  stored None. The repr shows the **original** (pre-normalization) string — after
  lossy decode it can contain U+FFFD, printed literally by repr.
- Validity: `normalize_id(v) = v.strip().upper()` (Python **Unicode** strip and
  upper — `'ß'.upper() == 'SS'`), then regex
  `^[A-Z][A-Z0-9]{1,9}-[0-9A-HJKMNP-TV-Z]{12}$` (`re.match`; suffix is Crockford
  base32 — no I, L, O, U). Stored value is the **normalized** id. Verified:
  `rac-ktq63dpsmf19` and `'  rac-ktq63dpsmf19  '` → `RAC-KTQ63DPSMF19`;
  `RAC-KTQ63DPSMFI9` (contains I) → invalid-id-syntax.

**type** (optional; `type: null` == absent):
- must be a string AND a registered artifact type; otherwise one issue:
  `invalid-metadata-field / frontmatter field 'type' is not a registered artifact type: {value!r}`;
  stored None. Verified reprs: `'banana'`, `5`, `9`.
- Registered names, exact case-sensitive match, in registry order:
  `requirement`, `decision`, `roadmap`, `prompt`, `design`
  (`ARTIFACT_SPECS` in `src/rac/core/artifacts.py`; `Decision` ≠ `decision`,
  capitalized → error. UNVERIFIED for the capitalized case specifically, but
  `spec_for` is a plain `==` loop).

**relationships** (optional; `relationships: null` == absent → `{}`, no issue):
- Well-formed iff it is a mapping whose every key is a str and every value is a
  list of str. Empty mapping `{}` is fine. Any violation (non-map, scalar target,
  non-str element, non-str kind) → single issue:
  `invalid-metadata-field / frontmatter field 'relationships' must map relationship kinds to lists of artifact IDs`;
  stored `{}`. Note YAML-1.1 resolution bites here: an unquoted target that looks
  like a date/int/bool makes the whole field malformed.
- On success every target is `normalize_id`-ed (`strip().upper()`) with **no
  validity check** — `'  not an id '` → `'NOT AN ID'` passes through (verified).
  Kind order and duplicate targets are preserved.

**tags** (optional; `tags: null` == absent → `[]`, no issue):
- Well-formed iff a list where every element is a str with truthy `.strip()`
  (Unicode strip — a tag of only NBSP is "empty"). Empty list OK. Any violation
  (`''` element, whitespace-only element, non-list, non-str element e.g. an
  unquoted date) → single issue:
  `invalid-metadata-field / frontmatter field 'tags' must be a list of non-empty strings`;
  stored `[]`.
- On success elements are stored **stripped**: `[' Alpha ', beta]` → `['Alpha', 'beta']`.

---

## 6. Verified test matrix (input `raw` → outcome)

| raw | outcome |
|---|---|
| `a: yes` / `No` / `OFF` / `on` / `TRUE` / `false` | True/False/False/True/True/False |
| `a: y`, `a: n`, `a: none`, `a: inf` | strings |
| `a: ~` `null` `NULL` `Null`, empty | None (all) |
| `a: 0x1F` `010` `0b101` `1_000` `1:30` `+5` `-0` | 31, 8, 5, 1000, 90, 5, 0 |
| `a: 1e5`, `a: 1e+5` | **strings** |
| `a: .inf` `-.Inf` `.nan` `6.` `1_0.5` `1:30.0` | inf, -inf, nan, 6.0, 10.5, 90.0 |
| `a: 2026-07-11` | `datetime.date` → field-type errors downstream |
| `a: 2026-7-1` | string |
| `a: 1` + `a: 2` | duplicate-frontmatter-key `'a'` |
| `1: x` + `true: x` | duplicate-frontmatter-key `True` |
| `1: a` + `1.0: b` | duplicate-frontmatter-key `1.0` |
| nested map dup (`a:\n  b: 1\n  b: 2`) | duplicate-frontmatter-key `'b'` |
| `a: &x 1\nb: *x` | malformed: aliases not permitted |
| `a: &x 1\nb: 2` | OK (anchor without alias) |
| `<<:` anywhere, `=:` key | malformed: no constructor for merge/value tag |
| `? [1, 2]\n: x` | **uncaught `TypeError: unhashable type: 'list'`** |
| 30-deep nesting under one key | OK; 31-deep → malformed 32-level-cap message |
| raw of 65536 UTF-8 bytes | passes cap (then whatever YAML says) |
| raw of 65537 UTF-8 bytes | malformed: `frontmatter exceeds the 65536-byte cap` |
| top-level list / scalar / empty / comments-only | malformed: must be a YAML mapping… |

---

## 7. `parse_file` interplay (`src/rac/core/markdown.py`, in-scope excerpts)

`parse_file(path)` (all issues below land in `product.parse_issues`, and the
product is a degraded empty `Product(title=None)` on the error paths):

1. `cap = max_file_bytes()`: `DECIDED_MAX_FILE_BYTES` env override; unparseable
   (`int()` fails) or ≤ 0 → default `1048576`. Verified: `"abc"` falls back.
2. `os.path.getsize(path)`; if `> cap` → oversize (below). Then `open(path,"rb")`
   and `read(cap + 1)`; if the read returned `> cap` bytes → same oversize issue.
   Oversize (note **file** cap wording):
   `error / artifact-oversize / artifact exceeds the {cap}-byte file cap (set DECIDED_MAX_FILE_BYTES to raise it)` / line 1.
3. Any `OSError` from stat/open/read →
   `error / unreadable-artifact / cannot read artifact: {exc}` / line 1 — the
   sentinel embeds the OS error string verbatim, e.g.
   `cannot read artifact: [Errno 2] No such file or directory: '<path>'` and
   `[Errno 21] Is a directory: '<path>'` (verified; strings are
   platform/locale-dependent — parity harness should treat the suffix as
   platform-defined).
4. `data.decode("utf-8")` strict; on `UnicodeDecodeError`, re-decode with
   `errors="replace"` and append (AFTER parsing, so it is the **last** parse
   issue): `warning / non-utf8-content / artifact is not valid UTF-8; decoded lossily` / line 1.
   Replacement semantics = Python/WHATWG UTF-8: each maximal invalid subsequence →
   **one U+FFFD per bogus byte** for stray continuation/invalid bytes, and one
   U+FFFD for a truncated multi-byte prefix (verified:
   `b"a\xff\xfe\xf0\x9f b" → "a��� b"` — `\xf0\x9f` truncated
   prefix collapses to a single U+FFFD). Rust's `String::from_utf8_lossy` follows
   the same policy. The U+FFFDs then flow into frontmatter values and appear
   inside issue-message reprs (verified: an id containing `\xff` reports
   `invalid artifact ID syntax: 'RAC-KTQ63DPSM�19 (…)'` with a literal � —
   repr does not escape it).
5. `parse(text)` itself first applies the **parse** cap with different wording:
   `error / artifact-oversize / artifact exceeds the {cap}-byte parse cap (set DECIDED_MAX_FILE_BYTES to raise it)` / line 1
   — "file cap" vs "parse cap" wording is pinned, do not unify (verified both).
   This runs *before* `split_frontmatter`, so a >cap text never reaches
   frontmatter parsing.

## 8. Unterminated block (emitted by `parse`, not `parse_frontmatter`)

When `split.raw is None and split.unterminated` — `parse` appends to
`metadata_issues`:

```
error / malformed-frontmatter / frontmatter block opened with --- on line 1 but never closed / line 1
```

and the whole text (including the `---` line) is parsed as body. Verified.

---

## 9. UNVERIFIED / open items

- `RecursionError` fallback message (`"frontmatter nesting too deep to parse"`) —
  believed unreachable behind the depth cap.
- `MarkedYAMLError` with `problem=None` would print
  `frontmatter is not valid YAML: None` — no known trigger.
- U+200B in the delimiter line (reasoned from `str.isspace()`, not run).
- Capitalized `type: Decision` rejection (reasoned from `spec_for`'s `==` loop).
- Exhaustive PyYAML scanner/parser problem-string catalog: only the samples in
  §3c were captured. Full byte parity for arbitrary malformed YAML requires
  porting PyYAML's message strings wholesale; recommend the fuzz phase diff
  Rust-vs-oracle on the `malformed-frontmatter` message text specifically.
- Whether any CLI surface ever prints `Issue.line` for frontmatter issues with an
  offset applied — out of scope here (all module-level lines are `None`).
