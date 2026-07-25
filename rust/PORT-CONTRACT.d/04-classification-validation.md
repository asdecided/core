# 04 — Classification, Identity, and Validation

Source modules: `src/rac/core/classification.py`, `src/rac/core/artifacts.py`,
`src/rac/core/identity.py`, `src/rac/core/idgen.py`, `src/rac/core/metadata.py`,
`src/rac/core/validation.py`, `src/rac/core/schema.py`, `src/rac/core/overrides.py`.
Upstream inputs (`Product`, section map) come from the parser (`markdown.py`,
`frontmatter.py`) — see the parser contract section; the `## section` normalization
and requirement extraction rules that feed classification/validation are summarized
where load-bearing here.

Everything in this section is **pure and deterministic** (ADR-002). No clock, no
git, no filesystem except `idgen`'s injected clock/entropy. Given the same
`Product`, every function here returns byte-identical results.

---

## 0. Prerequisite: how `Product.sections` is built (parser contract, load-bearing)

Classification and validation read `product.sections`, a `dict[str, str]` mapping
**normalized `##` heading → joined body text**, *in document order* (Python dict
preserves insertion order — the Rust port MUST use an insertion-ordered map).

- **Heading normalization** = `text.strip().casefold()`. Only `##` (h2) headings
  become section keys. h1 is the title; h3+ mark body as `"other"` (captured
  generically but never a recognized section).
  - `.strip()` = Python `str.strip()`: removes leading/trailing Unicode whitespace
    (the full `str.isspace()` set, incl. `\t \n \r \f \v`, NBSP U+00A0? — NO: NBSP
    is NOT stripped by `str.strip()`; but U+2028, U+2000–U+200A, U+3000, etc. ARE).
    **LANDMINE:** Rust `str::trim()` uses `char::is_whitespace` (Unicode
    White_Space property) which is close but NOT identical to Python's
    `str.isspace()`. Notably Python `str.isspace()` treats U+001C–U+001F (FS/GS/RS/US)
    and U+0085 (NEL) as space; Rust `is_whitespace` treats U+0085 as whitespace but
    NOT U+001C–U+001F. Verify each boundary char explicitly.
  - `.casefold()` = full Unicode case folding (more aggressive than `.lower()`):
    e.g. `"ß".casefold() == "ss"`, `"İ".casefold()`, Greek final sigma, etc.
    **LANDMINE:** Rust has no stdlib `casefold`. `str::to_lowercase` is NOT the
    same (`"ß".to_lowercase() == "ß"`). Use a Unicode case-folding crate matching
    Python's `str.casefold()` (Unicode `CaseFolding.txt`, full folding, default
    (non-Turkic) rules). All section matching, synonym lookup, metadata-value
    comparison, and identifier de-duplication depend on this.
- An empty `##` section still appears in `sections` (heading recorded with `""`
  body). Presence keys off heading, not body.
- Interior `\r` is stripped from each captured body line (CRLF-safe). Blank body
  lines are dropped. Body lines are each `.strip()`ed then joined with `"\n"`.
- The typed recognized sections `problem/requirements/success_metrics/risks` are
  extracted separately (see parser contract); `has_*_section` booleans track
  heading presence.

`product.metadata` is `None` for legacy (no-frontmatter) documents; otherwise an
`ArtifactMetadata` with `.id` already normalized to UPPERCASE (see §3).

---

## 1. ARTIFACT_SPECS — the schema registry

`ARTIFACT_SPECS` is an **ordered tuple** of 5 specs, in this exact order:
`requirement`, `decision`, `roadmap`, `prompt`, `design`. Order is load-bearing:
it is the tie-breaker in classification (§2), the order of `available_schemas()`,
and the iteration order for anything walking the registry.

Each `ArtifactSpec` (all section names are already-normalized lowercase strings):

| field | type | meaning |
|---|---|---|
| `name` | str | canonical key, e.g. `"requirement"` |
| `display` | str | human label, e.g. `"Requirement"` |
| `required` | tuple[str] | sections that define the type (scored) |
| `recommended` | tuple[str] | expected-but-optional (scored at 0.5) |
| `optional` | tuple[str] | recognized/extracted, never scored, never "missing" |
| `metadata` | dict[str, tuple[str]] | `{section → allowed values}` (case-insensitive) |
| `retired_status` | tuple[str] | subset of `metadata["status"]` marking retirement |
| `descriptions` | dict | schema-render hints (not used by classify/validate) |
| `guidance` | dict | improve/template hints (not used by classify/validate) |
| `synonyms` | dict[str,str] | alt heading → canonical, applied before matching |
| `id_field` | str \| None | canonical-id section; **no spec sets it today** (always None) |
| `starter_bodies` | dict | template render (not used by classify/validate) |

`expected` (property) = `required + recommended` (used for `missing` in scoring).

For **classification and validation parity**, only `required`, `recommended`,
`optional`, `metadata`, `retired_status`, and `synonyms` matter. `descriptions`,
`guidance`, `starter_bodies` affect only `rac schema`/`rac improve` output (a
different contract section). The full verbatim values follow.

### 1.1 requirement
- required: `("problem", "requirements")`
- recommended: `("success metrics", "risks", "assumptions")`
- optional: `("related decisions", "related roadmaps", "related prompts", "related designs", "related requirements", "related tickets", "verified by")`
- metadata: `{"status": ("Proposed", "Accepted", "Superseded", "Deprecated")}`
- retired_status: `("Superseded", "Deprecated")`
- synonyms: `{"success criteria": "success metrics", "kpis": "success metrics", "kpi": "success metrics"}`

### 1.2 decision
- required: `("context", "decision", "consequences")`
- recommended: `("status", "category", "alternatives considered")`
- optional: `("supersedes", "related requirements", "related roadmaps", "related designs", "related decisions", "related tickets", "applies to")`
- metadata: `{"status": ("Proposed", "Accepted", "Superseded", "Deprecated"), "category": ("Architecture", "Product", "Process", "Technical", "Other")}`
- retired_status: `("Superseded", "Deprecated")`
- synonyms: `{"alternatives": "alternatives considered", "options considered": "alternatives considered"}`

### 1.3 roadmap
- required: `("outcomes", "initiatives")`
- recommended: `("success measures", "assumptions", "risks")`
- optional: `("related decisions", "related requirements", "related prompts", "related designs", "related roadmaps", "related tickets")`
- metadata: `{"status": ("Planned", "Achieved", "Superseded", "Abandoned")}`
- retired_status: `("Superseded", "Abandoned")`
- synonyms: `{"success metrics": "success measures"}`  ← **artifact-scoped**; only normalizes when scoring against the roadmap spec.

### 1.4 prompt
- required: `("objective", "input", "instructions", "output")`
- recommended: `("constraints", "examples", "evaluation")`
- optional: `("related requirements", "related decisions", "related roadmaps", "related designs", "related tickets")`
- metadata: `{"status": ("Active", "Deprecated")}`
- retired_status: `("Deprecated",)`
- synonyms: `{"expected output": "output", "output specification": "output", "input specification": "input"}`

### 1.5 design
- required: `("context", "user need", "design", "constraints")`
- recommended: `("rationale", "alternatives", "accessibility", "style guidance", "open questions")`
- optional: `("related requirements", "related decisions", "related roadmaps", "related prompts", "related tickets")`
- metadata: `{"status": ("Proposed", "Accepted", "Superseded", "Deprecated")}`
- retired_status: `("Superseded", "Deprecated")`
- synonyms: `{}` (none)

**LANDMINE (synonyms are per-spec, applied only during that spec's scoring).**
`"success metrics"` is a canonical Requirement section, but under the *roadmap*
spec it is a synonym pointing to `"success measures"`. A document with only
`## Success Metrics` scores it as `success metrics` for the requirement spec and
as `success measures` for the roadmap spec. Do NOT build one global synonym map.

---

## 2. Classification (`classification.py`)

`CONFIDENCE_THRESHOLD = 0.5`.

### 2.1 `_mapped(product, spec) -> set[str]`
`{ spec.synonyms.get(h, h) for h in product.sections }` — the set of the
document's normalized headings with this spec's synonyms applied. A **set**, so
duplicates collapse (e.g. if both `## Success Metrics` and `## Success Measures`
exist under the roadmap spec, both map to `"success measures"` → one element).

### 2.2 `score_artifacts(product) -> list[TypeScore]`
For each spec in `ARTIFACT_SPECS` order:
- `mapped = _mapped(product, spec)`
- `matched_required = [s for s in spec.required if s in mapped]` (schema order)
- `matched_recommended = [s for s in spec.recommended if s in mapped]`
- `missing = [s for s in spec.expected if s not in mapped]` (required+recommended order)
- `points = len(matched_required) + 0.5 * len(matched_recommended)` (float)
- `ceiling = len(spec.required) + 0.5 * len(spec.recommended)` (float)
- `fit = points / ceiling if ceiling else 0.0`

**Float semantics:** `points`/`ceiling` are Python floats. `0.5 * n` is exact in
binary FP; sums of halves and integers are exact for these small magnitudes, and
the divisions land on representable values in the tested range, but the Rust port
MUST use `f64` and replicate the exact arithmetic (`len_req as f64 + 0.5 *
len_rec as f64`), then divide. `confidence` is later `round(fit, 2)` — see §2.4
for Python banker's rounding.

**Sort:** `scores.sort(key=lambda t: (t.fit, len(t.matched_required)), reverse=True)`.
Python's sort is **stable**, and `reverse=True` reverses the comparison but keeps
stability (equal-key elements retain their original `ARTIFACT_SPECS` order — it
does NOT reverse ties). So the ordering is:
1. higher `fit` first;
2. tie → more `matched_required` first;
3. tie → original `ARTIFACT_SPECS` order (requirement < decision < roadmap <
   prompt < design).

**LANDMINE:** A naive Rust `sort_by(|a,b| b.cmp(a))` on the whole tuple would
reverse ties too. Replicate exactly: sort ascending by `(fit, matched_required_len)`
with a **stable** sort, then reverse the whole vector — OR sort descending by key
but keep original index as a final ascending tie-break. The Python result is:
stable sort on key ascending, then `reverse=True` flips element order including
equal-key runs. Concretely: `sorted(..., key=k, reverse=True)` = elements ordered
by descending key, and among equal keys the ORIGINAL order is preserved (Python
guarantees this). Test: two specs with identical `(fit, matched_required_len)`
must come out in `ARTIFACT_SPECS` order.

### 2.3 `classify(product) -> Classification`
```
scores = score_artifacts(product)
best = scores[0] if scores else None   # always non-empty (5 specs)
if best is None or best.fit < 0.5 or not best.matched_required:
    return Classification(type="unknown",
                          confidence=round(best.fit,2) if best else 0.0,
                          present_sections=list(product.sections),   # all normalized headings, doc order
                          missing_sections=[])
return Classification(type=best.name,
                      confidence=round(best.fit,2),
                      present_sections=best.matched_required + best.matched_recommended,
                      missing_sections=best.missing)
```
Unknown when best fit < 0.5 **OR** zero required sections matched (even if fit
somehow ≥ 0.5 via recommended — not reachable given ceilings, but replicate the
`not best.matched_required` guard). Unknown is a **success**, not an error.

### 2.4 `round(fit, 2)` — Python banker's rounding
`confidence` = `round(fit, 2)`. Python 3 `round()` uses **round-half-to-even**
(banker's rounding), operating on the IEEE-754 double. **LANDMINE:** Rust
`(x*100.0).round()/100.0` uses round-half-away-from-zero and will diverge on
exact halves. Match Python `round(x, 2)` semantics: round to even on the .005
boundary, and note Python rounds the *actual double* (so `round(2.675,2)==2.67`
because 2.675 is stored as 2.67499...). For the fit values here the denominators
are `{1.0(req only n=?), ...}`; enumerate reachable fit values and pin
`confidence` per case. Reachable `fit` numerators/denominators are small rationals
(e.g. requirement ceiling = 2 + 0.5*3 = 3.5; a single required match → 1/3.5 =
0.2857… → round → 0.29). Confidence is display/JSON only — it does NOT affect the
chosen type — but it IS in JSON output bytes, so parity matters.

**`missing_sections` in the Unknown branch is always `[]`** (empty), and
`present_sections` is ALL headings. In the classified branch, `present_sections`
is only the matched required+recommended (synonym-canonical names), and
`missing_sections` is `best.missing`.

### 2.5 `missing_sections(product, spec)` (standalone helper)
Returns `(missing_required, missing_recommended)`, synonym-aware, schema order.
Independent of scoring. Used by `improve`, not by classify/validate. Include for
completeness.

---

## 3. Identity (`identity.py`, `metadata.py`)

### 3.1 Canonical opaque ID grammar (`metadata.ID_RE`)
```
^[A-Z][A-Z0-9]{1,9}-[0-9A-HJKMNP-TV-Z]{12}$
```
- Repository key: leading letter, then 1–9 more of `[A-Z0-9]` → **2–10 chars total**.
- Separator `-`.
- Suffix: exactly **12** chars of Crockford base32 UPPERCASE, excluding I, L, O, U
  (the char class `[0-9A-HJKMNP-TV-Z]` = 0-9, A-H, J, K, M, N, P-T, V-Z).
- `is_valid_id(v)` = `ID_RE.match(normalize_id(v))` where `normalize_id(v) =
  v.strip().upper()`. So matching is **case-insensitive** and surrounding
  whitespace is stripped before the test; the stored/returned id is UPPERCASE.
- `.upper()` is Python `str.upper()` (full Unicode; e.g. `"ß".upper()=="SS"`). For
  ASCII ids this is trivial, but a non-ASCII input could expand — the regex then
  fails it anyway. Match `str.strip()` + `str.upper()` semantics.

### 3.2 `artifact_identifier(product, spec, path) -> str` — precedence (first wins)
1. `product.metadata.id` if metadata present and id truthy (already UPPERCASE).
2. `## ID` section first value (casing preserved) — see `_first_value`.
3. `spec.id_field` section first value — but **no spec sets id_field**, so dead
   code today (still port the branch for fidelity).
4. filename-stem prefix matching `^[A-Za-z]+-\d+` (e.g. `adr-004` from
   `adr-004-parser-strategy`) — `prefix.group(0)`, casing preserved from filename.
5. whole filename stem (`Path(path).stem`).

`Path(path).stem` = filename without the LAST suffix. **LANDMINE:** Python
`Path("a.b.md").stem == "a.b"` (strips only final `.md`); `Path("a.").stem ==
"a."` (CORRECTION 2026-07-11, verified against the oracle interpreter: a
trailing dot is NOT a suffix — pathlib requires `0 < rfind('.') < len-1` —
so the stem keeps it; this file previously claimed `"a"`);
`Path(".hidden").stem == ".hidden"` (a leading-dot name has no suffix);
`Path("a/b/").stem == "b"`. Replicate `pathlib` stem semantics, not a naive
split-on-first-dot.

`_ID_PREFIX_RE = ^[A-Za-z]+-\d+` — one-or-more ASCII letters, `-`, one-or-more
ASCII digits. `\d` in Python `re` **without `re.UNICODE`... but Python 3 `re` is
Unicode by default**, so `\d` matches Unicode decimal digits (e.g. Arabic-Indic
U+0660). **LANDMINE:** `\d` here matches any Unicode `Nd` digit, not just `[0-9]`.
For a filename with fullwidth/other digits this matters. Rust `regex` crate `\d`
is ASCII-only by default and Unicode with a flag — set it to Unicode to match, or
enumerate. `[A-Za-z]` is ASCII-only (explicit class), so no Unicode letters.

`_first_value(body)`: first non-empty (`.strip()`) line, then strip ONE leading
list marker `^(?:[-*+]|\d+\.)\s+` (a `-`, `*`, `+`, or `N.` followed by
whitespace), then `.strip()` again. Note this `\d+\.` also matches Unicode digits.
Empty/None body → `""`.

### 3.3 `artifact_identifiers(...) -> list[str]` (all aliases, canonical first)
Order of consideration, each added only if truthy AND not already present
**case-insensitively** (`value.casefold() not in {i.casefold() for i in ids}`):
1. `metadata.id` (if present);
2. `_legacy_identifier` (`## ID` then `spec.id_field`);
3. filename-stem prefix (`^[A-Za-z]+-\d+`), if it matched;
4. whole filename stem.
De-dup uses **casefold** (§0 landmine applies). Duplicate-identity detection
elsewhere uses only element [0]; aliases never create duplicates alone.

### 3.4 `identity_conflict(product, spec) -> (fm_id, legacy_id) | None`
- None if no `metadata.id`.
- `legacy = _legacy_identifier(...)`; None if no legacy.
- None if `legacy.strip().upper() == metadata.id` (metadata.id already UPPERCASE;
  compare legacy uppercased-and-stripped). **Note asymmetry:** here legacy is
  compared with `.strip().upper()` (NOT casefold), while §3.3 de-dups with
  casefold. Match each site's exact method.
- else returns `(metadata.id, legacy)` (legacy casing preserved).

### 3.5 `idgen.generate_id` (ID minting — non-deterministic, injectable)
Not on the validation path; needed only if the port mints IDs. `<KEY>-<suffix>`,
suffix = 12 Crockford base32 chars: 8 chars from `int(clock()*1000) & (2^40-1)`
(big-endian base32, MSB first) + 4 chars from `entropy(20)` CSPRNG bits.
`ALPHABET = "0123456789ABCDEFGHJKMNPQRSTVWXYZ"`. `_encode(value, chars)` emits
`chars` symbols, `value & 0x1F` per step then `value >>= 5`, reversed (so
most-significant group first). Collision handling (regenerate on index hit) is the
caller's (`rac new`) job, not here.

---

## 4. Validation (`validation.py`) — the finding contract

`validate(product, *, ticketing_provider=None, artifact_type=None) -> list[Issue]`.
`Issue(severity, code, message, line=None)` where `severity ∈ {"error","warning"}`.

**`artifact_type` defaults to `classify(product).type`.** A caller may pass it in;
it MUST equal `classify(product).type` (byte-invisible either way).

### 4.1 Top-level dispatch and emission order (CRITICAL — the flat list order)
The returned list is built by concatenation **in this exact order**:
```
issues  = _validate_metadata(product, artifact_type)          # (A)
issues += _validate_ticketing_references(product, provider, artifact_type)  # (B)
then, by type:
  decision:    + _validate_decision(product)
  roadmap:     + _validate_roadmap(product)
  prompt:      + _validate_prompt(product)
  design:      + _validate_design(product)
  requirement: + _validate_requirement(product)
               + _validate_status_metadata(product, req_spec)
               + _validate_requirement_standards(product)
  unknown/other (fallback): + _validate_requirement(product)   # requirement rules ONLY
```
So the flat `issues` list is: **[metadata issues][ticketing issues][per-type
issues]**. This flat order is what the **directory JSON** (`files[].issues`) and
SDK callers see verbatim. The **single-file human/JSON CLI** re-groups into
`errors[]` then `warnings[]` (stable within each group) — that regrouping is a
CLI-render concern (see the CLI contract section), NOT this function.

**LANDMINE (Unknown ≠ requirement).** An Unknown document runs ONLY
`_validate_metadata` + `_validate_ticketing_references` + `_validate_requirement`
— NO status-metadata, NO requirement-standards (BCP-14/29148/EARS). Ticketing
lint also no-ops for Unknown because `spec_for("unknown")` is None. So an Unknown
doc gets: metadata issues, then the requirement structural rules (title,
missing-problem, missing-requirements, malformed lines, dup ids, warnings).

### 4.2 `_validate_metadata` (A)  — always runs, all types
```
issues = list(product.metadata_issues) + list(product.parse_issues)
spec   = spec_for(artifact_type)      # None for unknown
conflict = identity_conflict(product, spec)
if conflict: append Issue("error", "conflicting-identity", MSG)
```
- `metadata_issues` come from the frontmatter parser (see frontmatter contract):
  codes `malformed-frontmatter`, `duplicate-frontmatter-key`,
  `invalid-metadata-field`, `unsupported-schema-version`, `invalid-id-syntax`.
  Their **relative order is fixed by the parser**: unknown-fields → schema_version
  → id → type → relationships → tags. Envelope-fatal issues (malformed YAML,
  duplicate key, non-mapping, oversize) are a single terminal issue.
- `parse_issues`: `artifact-oversize`(error), `unreadable-artifact`(error),
  `field-truncated`(warning, sorted-heading order), `body-truncated`(warning),
  `non-utf8-content`(warning). See parser contract for messages.
- **conflicting-identity** message (verbatim, note the wrapping — it is ONE
  string, no newline; the Python source splits the literal across lines but they
  concatenate with single spaces):
  ```
  frontmatter id {frontmatter_id!r} conflicts with declared legacy identity {legacy_id!r}; align them — RAC will not choose one
  ```
  `{x!r}` = Python `repr()` (see §4.9). Note the Unicode em-dash `—` (U+2014).

### 4.3 `_validate_ticketing_references` (B)
No-op returning `[]` unless ALL hold: `provider` truthy and ≠ `"none"`;
`provider` is a known key in `TICKETING_PROVIDERS`; `spec` exists AND
`"related tickets" in spec.optional`. `provider` defaults to `None` on the pure
`validate` path — the service layer injects it from `.decided/config.yaml`
`ticketing.provider`. Providers and their entry validators:

| provider | key regex | URL accepted | label |
|---|---|---|---|
| `jira` | `^[A-Z][A-Z0-9]+-\d+$` | `^https?://\S+$` | `Jira key (e.g. PROJ-1234) or URL` |
| `github` | `^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+#\d+$` | same | `GitHub issue (e.g. owner/repo#123) or URL` |
| `linear` | `^[A-Z][A-Z0-9]*-\d+$` | same | `Linear key (e.g. ENG-123) or URL` |
| `azure-devops` | `^(?:AB#)?\d+$` | same | `Azure DevOps work item (e.g. 1234 or AB#1234) or URL` |
| `servicenow` | `^[A-Z]{2,}\d{5,}$` | same | `ServiceNow record (e.g. INC0010023) or URL` |

An entry is valid if `_URL_RE.match(entry) OR pattern.match(entry)`. For each line
of `product.sections["related tickets"]` (split on `\n`), strip the line, strip
ONE leading list marker `^(?:[-*+]|\d+\.)\s+`, strip again → `entry`; if `entry`
non-empty and invalid, append:
```
Issue("error", "malformed-ticket-reference",
      "## Related Tickets entry {entry!r} is not a valid {label}.")
```
Order = document line order. **Note `\d`/`\S` Unicode:** `\d` = Unicode digits,
`\S` = Unicode non-whitespace. `re.match` anchors at start only (not end) — but
these patterns all have `$`. `$` in Python matches end-of-string OR just before a
trailing `\n`. Since `entry` is stripped, no trailing newline; still, be aware
`$` ≠ `\z`. `_JIRA_KEY_RE` requires ≥2 leading key chars (`[A-Z][A-Z0-9]+`) while
`_LINEAR_KEY_RE` allows 1 (`[A-Z][A-Z0-9]*`).

### 4.4 `_validate_status_metadata(product, spec)` — constrained metadata enums
For each `(field_name, allowed)` in `spec.metadata.items()` (dict order = the
literal order written in the spec: for decision, `status` then `category`):
```
value = _first_value(product.sections.get(field_name, ""))   # local _first_value: first non-empty stripped line, NO list-marker strip
if value and not any(value.casefold() == a.casefold() for a in allowed):
    Issue("error", f"invalid-{spec.name}-{field_name}",
          f"## {field_name.title()} value {value!r} is not one of: {', '.join(allowed)}.")
```
- Comparison is **casefold-insensitive** against allowed values.
- Code examples: `invalid-decision-status`, `invalid-decision-category`,
  `invalid-requirement-status`, `invalid-roadmap-status`, `invalid-prompt-status`,
  `invalid-design-status`.
- Message uses `field_name.title()` → `"Status"`, `"Category"`. `str.title()` is
  Python's titlecase: capitalizes first letter of each run of cased chars,
  lowercases the rest. Single-word fields here → simple capitalize; but
  **LANDMINE** `str.title()` on multi-word or apostrophe'd strings misbehaves
  (`"user need".title() == "User Need"`, fine; `"it's".title()=="It'S"`). Only
  `status`/`category` reach this, both single ASCII words → `"Status"`/`"Category"`.
- `', '.join(allowed)` uses the spec's original casing/order, e.g.
  `Proposed, Accepted, Superseded, Deprecated`.
- **`_first_value` here (validation module) has NO list-marker stripping** — it is
  a different function from identity's `_first_value`. It returns the first
  non-blank line, `.strip()`ed. So `- Accepted` as a status line yields value
  `"- Accepted"` which then fails the enum check. Do not confuse the two helpers.

### 4.5 `_validate_title` (shared by decision/roadmap/prompt/design)
```
if not product.title:  Issue("error","missing-title","File has no top-level # title.")
if product.extra_title_lines:
    Issue("error","multiple-titles",
          "File has more than one top-level # title; expected exactly one.",
          product.extra_title_lines[0])   # line = first extra title line
```
`not product.title` is truthy when title is `None` OR `""` (empty). One
`multiple-titles` issue regardless of count; line points at the first extra.

### 4.6 `_validate_required_sections(product, spec)` (decision/roadmap/prompt/design)
For each `section in spec.required` (schema order), if `section not in
product.sections`:
```
Issue("error", f"missing-{section.replace(' ','-')}",
      f"{spec.name.title()} is missing a ## {section.title()} section.")
```
- Presence is checked against **raw normalized headings** (`product.sections`),
  **synonyms NOT applied** (synonyms are a classification aid only; validation
  wants the canonical heading). E.g. a Prompt with `## Expected Output` classifies
  as prompt but STILL fails `missing-output`.
- Code hyphenates spaces: `missing-user-need`, `missing-alternatives-considered`
  (recommended sections aren't required, so only required ones appear: for design
  `missing-context`, `missing-user-need`, `missing-design`, `missing-constraints`).
- Message: `spec.name.title()` → `"Requirement"`, `"Decision"`, `"Roadmap"`,
  `"Prompt"`, `"Design"`. `section.title()` → e.g. `"User Need"`, `"Context"`.

### 4.7 Per-type validators (order of their internal appends)

**`_validate_decision`**: `_validate_title` → `_validate_required_sections` →
`_validate_status_metadata`. (context/decision/consequences required; status +
category enums.)

**`_validate_roadmap`**: `_validate_title` → `_validate_required_sections` →
horizon check → linkage warning → `_validate_status_metadata`.
- Horizon: `horizon = _first_value(sections.get("horizon",""))`; if `horizon` and
  `horizon.casefold() not in ("now","next","later")` and NOT
  `_QUARTER_RE.match(horizon)` where `_QUARTER_RE = ^Q[1-4]\s+\d{4}$`:
  ```
  Issue("error","invalid-roadmap-horizon",
        "## Horizon value {horizon!r} is not one of: now, next, later, or a quarter (e.g. Q3 2026).")
  ```
  (`## Horizon` is not a scored/spec section — validated only if present. `\s`/`\d`
  Unicode caveat applies: `Q3 2026` with a NBSP or Unicode digit behaves oddly.)
- Linkage (warning), emitted BEFORE status-metadata: if BOTH `"related
  requirements" not in sections` AND `"related decisions" not in sections`:
  ```
  Issue("warning","roadmap-no-advancement-link",
        "Roadmap links no ## Related Requirements or ## Related Decisions it advances.")
  ```

**`_validate_prompt`**: `_validate_title` → `_validate_required_sections` →
`_validate_status_metadata`. (objective/input/instructions/output required.)

**`_validate_design`**: `_validate_title` → `_validate_required_sections` →
`_validate_status_metadata`. (context/user need/design/constraints required.)

### 4.8 `_validate_requirement` (+ standards, for `requirement` type and Unknown fallback)
`_validate_requirement` append order:
1. `_validate_title` (missing-title / multiple-titles)
2. `if not product.has_problem_section: Issue("error","missing-problem","File is missing a ## Problem section.")`
3. `if not product.has_requirements_section: Issue("error","missing-requirements","File is missing a ## Requirements section.")`
4. `_malformed_requirement_issues` (document order, one per malformed line):
   - `m.bad_id is None` → `Issue("error","req-missing-id", f"Requirement line has no [REQ-NNN] ID: {m.raw!r}", m.line)`
   - `elif m.empty_text` → `Issue("error","empty-req-text", f"Requirement [{m.bad_id}] has no description text.", m.line)`
   - `else` → `Issue("error","malformed-req-id", f"Malformed requirement ID [{m.bad_id}]; expected form [REQ-NNN].", m.line)`
5. `_report_duplicates` by `r.id`, severity error, code `duplicate-req-id`,
   message `f"Duplicate requirement ID {r.id} (used {n} times)."`, reported at the
   FIRST occurrence of each duplicated id, in document order.
6. `_requirement_warning_issues` (all warnings, in this sub-order):
   a. `if not product.has_metrics_section: Issue("warning","missing-success-metrics","No ## Success Metrics section (optional, but recommended).")`
   b. `if not product.has_risks_section: Issue("warning","missing-risks","No ## Risks section (optional, but recommended).")`
   c. `if product.has_problem_section and not (product.problem or "").strip(): Issue("warning","empty-problem","## Problem section is empty.")`
   d. `if len(product.requirements) > 50: Issue("warning","too-many-requirements", f"{n} requirements (more than 50); consider splitting the feature.")` (MAX_REQUIREMENTS=50; strictly greater)
   e. duplicate-req-text: `_report_duplicates` by `r.text.strip().casefold()`,
      severity warning, code `duplicate-req-text`, message
      `f"Duplicate requirement text: {r.text!r}."` (note: message uses raw
      `r.text` repr, but the DEDUP KEY is `r.text.strip().casefold()`).
   f. `_ambiguous_verb_issues`.

Then (requirement type only, appended by `validate`): `_validate_status_metadata`
(→ `invalid-requirement-status` if `## Status` present & bad) then
`_validate_requirement_standards`.

**`_ambiguous_verb_issues`**: for each requirement, `found =
_AMBIGUOUS_RE.findall(r.text)`, `_AMBIGUOUS_RE = \b(support|handle|allow|enable)\b`
case-insensitive. If any:
```
verbs = ", ".join(sorted({v.lower() for v in found}))
Issue("warning","ambiguous-verb", f"{r.id} uses ambiguous verb(s) ({verbs}); be more specific.", r.line)
```
- `\b...\b` word boundaries → **plural/inflected forms do NOT match**: `supports`,
  `handles`, `enabled`, `allowing` all fail (a word char follows the stem).
  Verified: `MUST support export` → matches; `supports and handles stuff` → no
  match. This is a high-value fuzz target.
- `sorted({v.lower()...})` → unique, lowercased, sorted (`allow, enable, handle,
  support` alphabetical). `\b` boundaries are Unicode-aware in Python `re`.

**`_validate_requirement_standards`** (requirement type only). For each
requirement, `keywords = _NORMATIVE_RE.findall(r.text)`, `_NORMATIVE_RE =
\b(shall|must|should)\b` IGNORECASE. Appends (in this per-requirement order,
looping requirements in document order):
1. `ambiguous = sorted({k for k in keywords if k != k.upper()})`; if non-empty:
   ```
   Issue("error","requirement-normative-keyword",
         f"{r.id} uses non-normative {', '.join(ambiguous)!r}; only uppercase MUST/SHALL/SHOULD/MAY carry normative weight (BCP 14).")
   ```
   at `r.line`. **LANDMINE — the `!r` wraps `', '.join(ambiguous)`**, i.e. the
   comma-joined string is repr'd as ONE string: e.g. keywords `{must,shall}` →
   `', '.join(['must','shall'])` = `"must, shall"` → `!r` → `'must, shall'`
   (single quotes around the whole thing, ONE pair). It is NOT per-keyword
   quoting. `k != k.upper()`: a keyword is "ambiguous" unless fully uppercase, so
   `Shall`, `shall`, `sHALL` all flag; `SHALL` does not. `.upper()` full-Unicode.
2. `if len(keywords) > 1: Issue("warning","requirement-not-singular", f"{r.id} has {len(keywords)} normative keywords; a requirement should be singular (ISO/IEC/IEEE 29148).", r.line)`
3. EARS: `if not keywords: Issue("warning","requirement-non-ears", f"{r.id} has no normative keyword (SHALL/SHOULD/MAY); it does not state a testable requirement (EARS).", r.line)`
   `elif _EARS_IF_RE.search(r.text) and not _THEN_RE.search(r.text):`
   (`_EARS_IF_RE = ^\s*if\b` IGNORECASE, `_THEN_RE = \bthen\b` IGNORECASE)
   `Issue("warning","requirement-ears-clause", f"{r.id} opens with 'If' but has no 'then' response clause (EARS unwanted-behaviour pattern: If <condition> then <system> SHALL …).", r.line)`
   Note the ellipsis `…` is U+2026, and `behaviour` is British spelling.

Per-requirement, standards issues emit in order: normative-keyword (if any) →
not-singular (if >1) → non-ears/ears-clause (exactly one of these, or neither).

### 4.9 Python `repr()` semantics for `{x!r}` (pervasive — every `!r` message)
Many messages embed `{value!r}`. Python `repr()` of a `str`:
- Chooses `'...'` quotes; switches to `"..."` only if the string contains `'` but
  not `"`. If it contains both, uses `'...'` and backslash-escapes the `'`.
- Escapes `\`, and control chars: `\n`→`\n`, `\t`→`\t`, `\r`→`\r`, others as
  `\xHH` / `\uHHHH` / `\UHHHHHHHH`. Non-printable per Python's `str.isprintable`.
- Printable non-ASCII is shown **literally** (not escaped): `repr("café")` =
  `'café'`. But e.g. a zero-width space or control char is `​`-escaped.
**LANDMINE:** Rust `format!("{:?}", s)` is CLOSE but NOT identical: Rust escapes
differently (always double-quotes; escapes `"`; uses `\u{...}` form; escapes some
chars Python shows literally and vice-versa). You MUST implement a Python-`repr`-
compatible string formatter for `str` to get byte-identical messages. Affected
codes: `req-missing-id` (`{m.raw!r}`), `duplicate-req-text` (`{r.text!r}`),
`requirement-normative-keyword` (`{joined!r}`), all `invalid-*` enum/horizon
messages (`{value!r}`), `malformed-ticket-reference` (`{entry!r}`),
`conflicting-identity` (two `!r`), and frontmatter parser messages.

### 4.10 `has_errors(issues)` = any `issue.severity == "error"`. The CLI decides
pass/fail from this (a run "fails" iff any error-severity issue survives
overrides). `validate` itself never fails; it returns everything.

---

## 5. Severity overrides (`overrides.py`) — post-processing pass
Applied by the **service layer** (`validate_product` / `validate_corpus`), not by
`validate` itself, but it transforms the finding list before human/JSON/exit-code:
`apply_overrides(issues, artifact_type, overrides)`.
- No-op if overrides empty (default) → returns the SAME list object.
- `resolve_severity(base, code, type, ov)`: start `sev=base`; if
  `ov.types.get(type) == "warning"` and `sev == "error"` → `sev="warning"` (per-
  type ceiling downgrades errors only); then if `ov.rules.get(code)` is set →
  `sev = that` (per-rule wins over ceiling; value ∈ `error|warning|off`).
- `off` → finding dropped; else severity replaced (order preserved). Overrides
  come from committed `.decided/config.yaml` `validation.rules` / `validation.types`.
Deterministic given repo state. The Rust port replicates this to match exit codes.

---

## 6. Frontmatter metadata issues feeding §4.2 (summary; full detail in parser section)
`parse_frontmatter(raw)` issue order (fixed contract): **unknown-fields →
schema_version → id → type → relationships → tags**. Terminal (metadata=None)
cases each yield ONE issue: `malformed-frontmatter` (bad YAML / non-mapping /
oversize >64KiB / alias / depth>32 / recursion), `duplicate-frontmatter-key`.
Field issues: `invalid-metadata-field` (unknown field, or wrong-shape
schema_version/id-type/type/relationships/tags), `unsupported-schema-version`
(int but ∉ `(1,)`), `invalid-id-syntax` (fails §3.1 grammar). Supported fields:
`("schema_version","id","type","relationships","tags")`. `schema_version`
required; a bool is rejected (`isinstance int and not bool`). `type` must be a
registered artifact name (`spec_for` non-None). Verbatim messages are in the
parser/frontmatter contract section — cross-reference; they appear in
`product.metadata_issues` and flow through `_validate_metadata` unchanged.

---

## 7. Verified empirically (oracle CLI, `.venv-oracle/bin/rac validate`)
- Directory JSON `files[].issues` preserves the raw flat `validate()` order
  (interleaved severities): confirmed `[warnings from _requirement_warning_issues]`
  precede `[requirement-normative-keyword (error), requirement-not-singular]` for a
  requirement doc — i.e. NOT grouped by severity.
- Single-file JSON regroups into `errors[]` then `warnings[]`, stable within group
  (a CLI-render transform, not `validate`).
- Ambiguous-verb boundary: `MUST support export` → `ambiguous-verb (support)`;
  `supports and handles stuff` → no ambiguous-verb (plurals don't match `\b...\b`).
- `duplicate-req-id` message `Duplicate requirement ID REQ-001 (used 2 times).`
  reported at first occurrence line; `malformed-req-id` message
  `Malformed requirement ID [REQ-1A]; expected form [REQ-NNN].`.
- Two lowercase normatives on one line → one `requirement-normative-keyword`
  (error) with joined-repr, plus `requirement-not-singular` (warning), same line.

## 8. UNVERIFIED / open
- Exact `round(fit,2)` half-to-even outputs per reachable fit value — enumerate and
  pin in a golden table (recommend generating from the oracle).
- Precise `str.strip()`/`str.casefold()`/`str.upper()`/`str.title()` divergence
  from Rust across the full Unicode boundary set — needs a differential fuzz corpus.
- `Path.stem` edge cases (trailing dot, multi-dot, dotfiles) not exercised against
  the oracle here — pin with a golden table.
- Whether any live corpus artifact sets `spec.id_field` (currently always None) —
  confirmed None in source; treat branch as dead but port it.
