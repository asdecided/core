# Port Contract 05 — Relationships (`rac relationships`)

Source of truth: `src/asdecided/services/relationships.py` (1477 LOC), plus its pure
foundation `src/asdecided/services/references.py`, the edge schema
`src/asdecided/core/relationship_types.py`, identity `src/asdecided/core/identity.py`,
scope helpers `src/asdecided/services/scope_paths.py`, and the renderers
`src/asdecided/output/{human,json,sarif}.py`. CLI wiring: `cmd_relationships` in
`src/asdecided/cli.py`.

This section specifies the `rac relationships` command end to end: how
relationship sections become typed edges, how references are extracted and
resolved, the human / `--json` / `--sarif` output byte layout and ordering, and
every `--validate` issue type with its exact message strings and exit codes.

Cross-references: corpus walk order and `find_markdown_files` are owned by the
FS/corpus brief; frontmatter `id` parsing by the frontmatter brief; markdown
section parsing by the markdown brief. This section depends on their behavior and
calls out the seams.

---

## 1. CLI surface

Subparser `relationships` (`cli.py` ~1692):

```
rac relationships <path> [--validate] [--sarif] [--top-level] [--recursive] [--json]
```

- `path` (positional, required): a directory to scan, or a single Markdown file.
- `--validate` (store_true): resolve references; switch to validation mode.
- `--sarif` (store_true): emit SARIF 2.1.0. **Requires `--validate`**.
- `--top-level` (store_true): directory mode only; disable recursion.
- `--recursive` (store_true): **no-op**, accepted for clarity. Recursion is the
  default; only `--top-level` disables it. If both `--top-level` and
  `--recursive` are given, `--top-level` wins (`recursive = not args.top_level`).
- `--json`: global flag (defined on the top-level parser, not here).

### 1.1 Path dispatch (`cmd_relationships`, cli.py 528-579)

Order of checks, exactly:

1. `if args.sarif and not args.validate:` → `_usage_error("relationships --sarif requires --validate")`.
   Prints `rac: relationships --sarif requires --validate\n` to **stderr**, exit **2**.
2. `path = Path(args.path)`.
3. `if path.is_dir():` → directory mode (`is_dir = True`).
4. `elif path.is_file():` → if `path.suffix.lower() not in (".md", ".markdown")`:
   `_usage_error(f"relationships expects a Markdown file or directory; convert it first with: rac ingest {args.path}")`
   → stderr `rac: relationships expects a Markdown file or directory; convert it first with: rac ingest <path>\n`, exit **2**.
   Otherwise single-file mode (`is_dir = False`).
5. `else:` → `_usage_error(f"path not found: {args.path}")` → stderr
   `rac: path not found: <path>\n`, exit **2**.

Note `is_dir()` is tried **before** `is_file()`. `suffix.lower()` — the extension
check is case-insensitive (`.MD`, `.Markdown` accepted). Directory-mode file
discovery, however, only matches `*.md` (see §2.1), so a `.markdown` file inside a
directory is *not* inspected; only a directly-named `.markdown` file is.

### 1.2 Output selection (`_emit`, cli.py 186)

Precedence ladder: `--sarif` (only reachable with `--validate`) → `--json` →
human. Each branch does `print(render())`, i.e. rendered string **+ trailing
`\n`**. If both `--json` and `--sarif` given with `--validate`, SARIF wins.

### 1.3 Exit codes

- Non-validate mode: **always 0** (`EXIT_OK`). Finding zero relationships is success.
- Validate mode: `EXIT_OK` (0) if `report.ok` (no issues) else `EXIT_VALIDATION_FAILED` (1).
- Usage errors above: `EXIT_USAGE` (2).

`EXIT_OK=0`, `EXIT_VALIDATION_FAILED=1`, `EXIT_USAGE=2` (cli.py 156-158).

---

## 2. Corpus items and ordering

### 2.1 Directory walk

Directory mode → `build_relationship_report(directory, recursive=not top_level)`
→ `_corpus_items` → `walk_corpus(directory, recursive)` → `find_markdown_files`
(`core/fs.py`):

```python
root = Path(directory)
glob = root.rglob if recursive else root.glob
found = [p for p in glob("*.md")
         if not any(part.startswith(".") for part in p.relative_to(root).parts)]
return sorted(found)
```

- Only `*.md` (not `.markdown`) in directory mode.
- Any path component (relative to root) starting with `.` is skipped (`.git`,
  `.venv`, `.decided`, dotfiles). The check is on `relative_to(root).parts`, so the
  root directory's own name is not tested.
- **`sorted(found)`** — Python sorts `Path` objects by their string tuple parts.
  For output ordering, path strings are `str(entry.path)`, which is the
  `directory`-arg-prefixed path (e.g. arg `rac/decisions` → `rac/decisions/adr-001-….md`).
  Python `sorted()` on strings is by Unicode code point; UTF-8 byte order equals
  code point order for well-formed UTF-8, so Rust `Vec<String>::sort()` (byte
  order) matches **only if paths are compared as the same normalized string
  form**. Match `pathlib.Path` string form: `Path` collapses `foo//bar`→`foo/bar`
  and strips a trailing slash on a component, but preserves the raw arg prefix
  otherwise. Verify against the FS brief.

Every artifact in output preserves snapshot (sorted-path) order throughout:
per-artifact report list, validation reference issues, resolve rows.

### 2.2 Single-file

Single-file mode → `build_relationship_report_file(path)` /
`validate_relationships_file(path)` → `_parsed_items([path])`, one item,
`recursive=False`. The identifier index then contains **only this file**, so
cross-file references never resolve (all become `relationship-target-not-found`
or resolve to self); labels never appear.

### 2.3 `directory` field is the **raw arg**

`report.directory = args.path` verbatim (trailing slash preserved). Empirically:
arg `"rac/decisions/"` → JSON `"directory": "rac/decisions/"` but member paths are
`"rac/decisions/adr-….md"` (no double slash — `Path`/rglob normalized).

### 2.4 Per-item projection

Each item is `(path_str, product, spec)` where
`spec = spec_for(classify(product).type)`. `spec is None` for Unknown/untyped
documents. An Unknown document contributes its **identifiers** to the resolution
index (can collide → duplicate) but declares **no edges** and **no unsupported
sections**.

---

## 3. Relationship-section vocabulary and edge extraction

### 3.1 Canonical section list (`references.py`)

```
RELATED_SECTIONS  = ("related requirements", "related decisions",
                     "related roadmaps", "related prompts", "related designs")
EXTERNAL_SECTIONS = ("related tickets", "verified by")
SCOPE_SECTIONS    = ("applies to",)
RELATIONSHIP_SECTIONS = RELATED_SECTIONS + ("supersedes",) + EXTERNAL_SECTIONS + SCOPE_SECTIONS
```

So canonical order (index → snake key) is:
```
0 related requirements  related_requirements
1 related decisions     related_decisions
2 related roadmaps       related_roadmaps
3 related prompts        related_prompts
4 related designs        related_designs
5 supersedes             supersedes
6 related tickets        related_tickets
7 verified by            verified_by
8 applies to             applies_to
```

`_snake(section)` = `section.replace(" ", "_")` (spaces→underscores only).

### 3.2 Per-artifact edge order = `spec.optional` order (NOT canonical)

`extract_relationships_full(product, spec)` = `_collect(product, spec, RELATIONSHIP_SECTIONS)`:
iterate `spec.optional` in order; for each section that is (a) in the `allowed`
set and (b) present in `product.sections` with a non-empty body and (c) yields
≥1 parsed reference, emit `{_snake(section): [refs]}`. **The dict key order is
`spec.optional` order**, which differs per artifact type. First-seen-wins dict.

`spec.optional` per type (from `core/artifacts.py`):

| Type | `spec.optional` order |
|------|----------------------|
| requirement | related decisions, related roadmaps, related prompts, related designs, related requirements, related tickets, verified by |
| decision | supersedes, related requirements, related roadmaps, related designs, related decisions, related tickets, applies to |
| roadmap | related decisions, related requirements, related prompts, related designs, related roadmaps, related tickets |
| prompt | (see artifacts.py ~343) |
| design | (see artifacts.py ~434) |

So a decision's report dict lists `supersedes` first, then
`related_requirements`, etc. This ordering flows into: the JSON `relationships`
object, the human per-artifact section blocks, and the resolve-row edge order.

`extract_relationships` (used by `rac inspect`, not this command) excludes
`supersedes`; `extract_relationships_full` (this command) **includes**
`supersedes`.

### 3.3 Reference extraction (`parse_references`, references.py 75)

```python
_LIST_MARKER_RE = re.compile(r"^(?:[-*+]|\d+\.)\s+")
for line in body.splitlines():
    stripped = line.strip()
    if not stripped: continue
    references.append(_LIST_MARKER_RE.sub("", stripped, count=1).strip())
```

One reference per non-empty line. A **well-formed** leading list marker
(`-`, `*`, `+`, or `\d+.` followed by ≥1 whitespace) is stripped **once**
(`count=1`), then `.strip()` again. Otherwise the whole stripped line is the
reference verbatim. The line text **is** the reference — no ID parsing, no
resolution here (ADR-016).

**PYTHON-SPECIFIC LANDMINES (verify against markdown brief; these are load-bearing here):**

- `str.splitlines()` splits on a **broad** set of boundaries:
  `\n \r \r\n \v(\x0b) \f(\x0c) \x1c \x1d \x1e \x85(NEL)    `.
  Verified: `"a\x1cb"`, `"a\x85b"`, `"a b c"`, `"line\x0bwith\x0cvtab"`
  all split. Rust `str::lines()` splits **only** `\n` and `\r\n`. A Rust port
  MUST reimplement Python `splitlines()` exactly, or refs will merge/differ.
- `str.strip()` strips Python-whitespace = `str.isspace()` set, which includes
  `\x1c–\x1f`, `\x85`, `\xa0`(NBSP), ` `, ` `, plus ASCII. Verified:
  `"  \xa0 ADR-nbsp".strip()` → `"ADR-nbsp"`; `"x\x1f".strip()` → `"x"`. It does
  **not** strip `​` (zero-width space; `isspace()` False).
  Rust `str::trim()` uses Unicode `White_Space`, which INCLUDES `\x85 \xa0
     ` but EXCLUDES `\x1c–\x1f`. Divergence: `\x1c–\x1f` are stripped by
  Python but not by Rust `trim()`. Reimplement strip against Python's set.
- Regex `\s` in `_LIST_MARKER_RE` (str pattern) is Unicode-aware.
- `--ADR` → marker `-` not followed by whitespace → no strip → `"--ADR"` kept.
  `1.ADR` (no space after dot) → no strip → `"1.ADR"` kept.
  `-  spaced` → `"spaced"`. `10.  ADR-5` → `"ADR-5"`. `REQ-001 (blocked)` kept whole.
  `../path/to` kept whole (leading `..` is not a list marker).

### 3.4 `unsupported_relationship_sections` (references.py 152)

Iterate `RELATIONSHIP_SECTIONS` **in canonical order**; for each section
**NOT in `spec.optional`** that is present with ≥1 parsed reference, append the
**canonical (space) section name**. Returns canonical-order list. This drives the
`relationship-edge-unsupported` finding. (Only reached for `spec is not None`.)

---

## 4. Identity and the resolution index

### 4.1 `artifact_identifiers` (identity.py 85) — alias list, canonical first

Order, dedup case-insensitively (first casefold wins, casing of first occurrence kept):
1. `product.metadata.id` if set (frontmatter canonical id; already uppercased by
   frontmatter parser).
2. `_legacy_identifier`: first non-empty line of `## ID` section (marker-stripped,
   casing preserved); else `spec.id_field` section value if `spec` set.
3. filename-stem prefix matching `^[A-Za-z]+-\d+` (e.g. `adr-004` from
   `adr-004-parser-strategy`).
4. whole filename stem (`Path(path).stem`).

`_first_value` uses the same `_LIST_MARKER_RE` marker-strip + `.strip()` on the
first non-empty line.

`artifact_identifier` (single, identity.py 56): first match of
metadata.id → `## ID` → `spec.id_field` → stem-prefix → whole stem.

**LANDMINE:** frontmatter `id` parsing is strict — an unrecognized `id:` value
yields `metadata.id = None` (verified: `id: ADR-001` in a minimal doc gave
`metadata.id=None`, so only the stem indexed). Defer to the frontmatter brief for
the exact accepted id grammar; relationship resolution is only as good as the
identifiers it is fed.

### 4.2 Resolution index

`build_resolution_index(items)` → `{ident.casefold(): [(path, display_ident), …]}`.
Every alias of every item (Unknown included) is added, in item order, appending.
`resolution_index_from_rows` / `_index_from_resolve_rows` build the byte-identical
index over a row's `identifiers` tuple.

**LANDMINE — `str.casefold()`:** the index key and every lookup use Python
`str.casefold()`, full Unicode case folding (stronger than `.lower()`): `ß`→`ss`,
`ﬁ`→`fi`, Greek final sigma, Turkish dotted/dotless I, etc. Verified escaping
aside, Rust must use full Unicode case folding (e.g. the `caseless`/`unicode-case`
crate or `char::to_lowercase` is **not** equivalent — casefold ≠ lowercase). ASCII
identifiers are unaffected. Reference matching, duplicate detection, cycle
adjacency, scope — everything keys on `.casefold()`.

---

## 5. Report (non-validate) model

`RelationshipReport` (relationships.py 86). `_build_report`: for each item with a
non-None spec, `extract_relationships_full`; include the artifact only if it has
≥1 relationship section. `artifacts` preserves snapshot (sorted-path) order.

- `total_files` = `len(items)` (every walked file, incl. no-relationship and
  Unknown).
- `artifacts_with_relationships` = `len(artifacts)`.
- `counts` (property): aggregate reference counts by section, keyed in
  **canonical `RELATIONSHIP_SECTIONS` order**, zero-count sections omitted.
  ```python
  totals[section] += len(refs)   # summed across artifacts, per snake key
  return {snake(s): totals[snake(s)] for s in RELATIONSHIP_SECTIONS if snake(s) in totals}
  ```
- `relationship_count` = `sum(counts.values())`.
- `labels`: `{ref.casefold(): "<title> (<type> · <canonical>)"}` for each
  reference that resolves to **exactly one** path in the index. Built by
  `_resolution_labels`: first-seen ref wins (`if key in labels: continue`); a ref
  whose index entries span >1 distinct path gets **no** label; `type_name` =
  `spec.name` or `"unknown"`; the display is `f"{title or canonical} ({type_name} · {canonical})"`.
  **`labels` is presentation-only — it is NOT emitted in JSON.**

### 5.1 Human render (`render_relationships_human`, human.py 625)

Exact line layout (`\n`-joined; `print` adds final `\n`):
```
Relationships
<blank>
Files Inspected: {total_files}
Artifacts With Relationships: {artifacts_with_relationships}
Relationships Found: {relationship_count}
```
Then, **only if `counts` non-empty**:
```
<blank>
By Type:
- {Label}: {count}          # one per counts entry, canonical order
```
Then, for each artifact in `artifacts` (in order):
```
<blank>
{artifact.path}
  {Label}:                  # per section, spec.optional order
  - {ref}                   # or "  - {ref} — {resolved_label}" if labels.get(ref.casefold())
```
- `_bold("Relationships")` / `_bold("By Type:")` wrap in ANSI `\033[1m…\033[0m`
  **only when `sys.stdout.isatty()`** (`_USE_COLOR`). Piped output (the parity
  harness) has **no** escape codes.
- `_relationship_label(snake)` = `snake.replace("_", " ").title()`. Python
  `str.title()`: `related_decisions`→`Related Decisions`, `verified_by`→`Verified
  By`, `applies_to`→`Applies To`, `supersedes`→`Supersedes`. (No apostrophes/digits
  in these names, so `.title()`'s quirks do not bite here — but a Rust
  `.replace('_'," ")` + naive title-case must match `str.title()` word rules.)
- The resolved-label suffix separator is **` — `** = U+2014 EM DASH (bytes
  `E2 80 94`); the label body separator is **` · `** = U+00B7 MIDDLE DOT (bytes
  `C2 B7`). Emitted as raw UTF-8 in human output.

### 5.2 JSON render (`render_relationships_json`, json.py 240)

`json.dumps(payload, indent=2)`. Key order (insertion order):
```json
{
  "directory": <raw arg str>,
  "recursive": <bool>,
  "total_files": <int>,
  "artifacts_with_relationships": <int>,
  "relationship_count": <int>,
  "counts": { <snake>: <int>, ... },        // canonical order, zeros omitted
  "artifacts": [
    { "path": <str>, "type": <spec.name|"unknown">, "relationships": { <snake>: [<ref>, ...] } }
  ]
}
```
`type` is `artifact.type` = `spec.name` (only artifacts with a spec appear, so
never `"unknown"` in practice here — Unknown files carry no relationships and are
excluded). `relationships` inner key order = `spec.optional` order. **No `labels`.**

**JSON formatting invariants (Python `json.dumps(indent=2)`):**
- `indent=2`; item separator `","` + newline; key separator `": "`.
- `ensure_ascii=True` (default): **all non-ASCII escaped as `\uXXXX`**. Verified:
  ref `café-ADR` → `"café-ADR"`. **LANDMINE:** Rust `serde_json` emits raw
  UTF-8 by default; the port MUST ASCII-escape (`\uXXXX`, surrogate pairs for
  astral chars) to be byte-identical.
- No trailing spaces; no trailing newline inside the string (the CLI `print` adds
  exactly one).

---

## 6. Validation model (`--validate`)

`validate_relationships` (dir) / `validate_relationships_file` (file) →
`_validate` → `_validation_rows_from_items` → `validation_from_rows`
(relationships.py 710), the single core. Returns `RelationshipValidation`:
`directory`, `recursive`, `relationships_checked`, `issues`. `ok = not issues`.

### 6.1 Issue codes (stable, part of JSON contract)

```
duplicate-artifact-identifier   ISSUE_DUPLICATE_IDENTIFIER
relationship-target-not-found   ISSUE_TARGET_NOT_FOUND
relationship-target-ambiguous   ISSUE_TARGET_AMBIGUOUS
relationship-self-reference     ISSUE_SELF_REFERENCE
relationship-edge-unsupported   ISSUE_EDGE_UNSUPPORTED
relationship-target-superseded  ISSUE_TARGET_SUPERSEDED
relationship-target-type-mismatch  ISSUE_TARGET_TYPE_MISMATCH
relationship-cycle              ISSUE_RELATIONSHIP_CYCLE
applies-to-target-not-found     ISSUE_SCOPE_TARGET_NOT_FOUND
```

Intrinsic severity (`RELATIONSHIP_SEVERITY`, used only by SARIF level; all
findings fail `--validate` regardless):
- error: target-not-found, target-ambiguous, target-type-mismatch,
  relationship-cycle, duplicate-artifact-identifier, applies-to-target-not-found.
- warning: target-superseded, self-reference, edge-unsupported.

### 6.2 `ValidationRow` projection (relationships.py 443)

Per item: `path`, `spec_name` (`spec.name` or None), `canonical_id`
(`artifact_identifier`), `identifiers` (tuple), `retired`
(`_is_retired_artifact`), `unsupported_sections` (canonical-order tuple, empty if
spec None), `edges` (tuple of `(snake_section, (refs…))` in spec.optional order,
empty if spec None).

`_is_retired_artifact`: `spec.retired_status` truthy AND `## Status` first
non-empty line (`.strip()`) `.casefold()`-equals one of `spec.retired_status`
(case-insensitive). Decision/requirement retired states: `("Superseded",
"Deprecated")`. Roadmap has its own (see artifacts.py). Uses the **first non-empty
line** of the status body (inline first-line rule, not the whole body).

### 6.3 Emission order of `issues` (CRITICAL — this is the exact list order)

`validation_from_rows` appends in this fixed sequence:

1. **Duplicate identifiers** (repo-level). Build
   `{canonical_id.casefold(): [(path, canonical_id)…]}` (one entry per doc — only
   the canonical id can collide). For each key with >1 entry: `display` =
   `min(entries, key=lambda e: e[0])[1]` (the display id from the
   **lexicographically smallest path**); `paths` = `sorted(paths)`. Collect, then
   emit **sorted by `display.casefold()`**. Fields: `identifier`, `paths`, `code`.

2. **Edge-unsupported** (only rows with `spec_name`). For each row (snapshot
   order), for each `section` in `row.unsupported_sections` (canonical order),
   emit `code`, `source_path=row.path`, `relationship=_snake(section)`.

3. **Range / type-mismatch** (`relationship-target-type-mismatch`). For each row
   (snapshot order, spec not None), each `(section, refs)` in `row.edges`
   (spec.optional order), each `ref` (declared order): skip if `edge is None or
   edge.external`; resolve via `_resolved(ref, source)` (unique non-self target
   only); if target's `spec_name` is None skip (untyped, not a violation); if
   `target_spec_name not in edge.range`, emit `source_path`, `relationship=section`,
   `target=ref`, `code`.

4. **Status-consistency / superseded** (`relationship-target-superseded`). For
   each row (spec not None **and not retired** — retired sources are exempt), each
   edge/ref: skip if `edge is None or edge.external or not edge.forbids_target_status`
   (so `supersedes` and all external edges never fire); if `_resolved` target is
   `retired`, emit `source_path`, `relationship=section`, `target=ref`, `code`.

5. **Cycles** (`_cycle_issues`). For each acyclic edge kind `kind` in
   `sorted(name for name,edge in REGISTRY.items() if edge.acyclic)` (today only
   `supersedes`): build adjacency `{source: sorted(unique non-self resolved
   targets)}` over that kind's edges; Tarjan SCC (nodes and neighbours visited in
   sorted order) collecting components of size >1 as **sorted node lists**;
   components returned `sorted(key=lambda c: c[0])`. One issue per component:
   `relationship=kind`, `paths=component`, `code`.

6. **Referential integrity** (`_resolve_references`). For each row (spec not
   None), each edge — **skip external edges** (`edge.external`, i.e.
   `related_tickets`, `verified_by`, `applies_to`) — each ref (declared order):
   `checked += 1`; `targets = index.get(ref.casefold(), [])` paths;
   - empty → `relationship-target-not-found`
   - >1 distinct entry → `relationship-target-ambiguous`
   - `targets == [row.path]` (resolves only to self) → `relationship-self-reference`
   - else resolved uniquely → no issue.
   Emit `source_path`, `relationship=section`, `target=ref`, `code`.
   `relationships_checked` = this `checked` count (external + scope refs are
   NEVER counted).

7. **Scope existence** (`_scope_validation_issues`, appended **last**). For each
   row (spec not None), each edge with `edge.filesystem_scoped` (only
   `applies_to`), each ref: only `classify_scope_entry(ref) == "path"` entries are
   checked; `normalized = normalized_scope_path(ref)`; if `normalized is not None
   and (root/normalized).exists()` → OK; else emit
   `applies-to-target-not-found` with `source_path`, `relationship=section`,
   `target=ref`. `root = repository_root(directory)`.

**Consequence:** within one `issues` list the group order is fixed
(duplicates → unsupported → type-mismatch → superseded → cycle → not-found/
ambiguous/self → scope). Verified empirically (rel2 corpus produced exactly:
duplicate, type-mismatch, superseded, self-reference, ambiguous). Note within
group 6 the not-found/ambiguous/self are interleaved in edge/ref traversal order,
not grouped by code.

### 6.4 `edge_spec` / `EdgeSpec` registry (relationship_types.py)

`REGISTRY` keyed by snake edge name. `edge_spec(name)` → EdgeSpec or None (a
section with no registry entry — none today, but defensive).

| edge | range | acyclic | forbids_target_status | external | external_provider | filesystem_scoped | directional |
|------|-------|---------|----------------------|----------|-------------------|-------------------|-------------|
| related_requirements | (requirement,) | F | T | F | F | F | F |
| related_decisions | (decision,) | F | T | F | F | F | F |
| related_roadmaps | (roadmap,) | F | T | F | F | F | F |
| related_prompts | (prompt,) | F | T | F | F | F | F |
| related_designs | (design,) | F | T | F | F | F | F |
| supersedes | (decision,) | **T** | **F** | F | F | F | T |
| related_tickets | () | F | T | **T** | **T** | F | F |
| verified_by | () | F | T | **T** | F | F | T |
| applies_to | () | F | T | **T** | F | **T** | T |

Enforced fields: `range` (type-mismatch), `acyclic` (cycle),
`forbids_target_status` (superseded), `external` (skip resolution/range/status),
`filesystem_scoped` (scope existence). `directional/symmetric/inverse/cardinality`
are declared-only.

### 6.5 `_resolved` / `_resolve_references` helpers

`_resolved(ref, source)` → unique non-self target path or None
(`len(targets)!=1 or targets[0]==source` → None). Ambiguity is by **distinct
path count in the index entries**, i.e. `[p for p,_ in index.get(key,[])]` — a
single artifact appearing multiple times under one alias key cannot happen
(identifiers dedup), but two artifacts sharing an alias makes it ambiguous.

### 6.6 Scope path helpers (scope_paths.py)

- `classify_scope_entry(entry)`: if any of `* ? [` in entry → `"glob"`; elif `/`
  in entry → `"path"`; else `"component"`. Only `"path"` entries are
  existence-checked. Separator is `/` **only** (not `\`).
- `normalized_scope_path(entry)`: `.strip()`; empty or startswith `/` → None
  (absolute rejected); split `PurePosixPath(text).parts`, drop `.`, any `..` →
  None (escape rejected); join remaining with `/`; None if empty.
- `repository_root(directory)`: `Path(directory).resolve()`, walk up
  `(resolved, *parents)`, return first dir containing `.decided/config.yaml`, else the
  resolved directory. **In this repo `.decided/config.yaml` exists at root**, so
  `applies_to` paths like `src/asdecided/` resolve against the repo root. In an
  un-initialized tree the root is the resolved arg dir. `(root/normalized).exists()`
  follows symlinks and matches file OR directory.

**LANDMINE:** scope existence checks touch the **real working tree**, so scope
findings depend on filesystem state and on `repository_root` discovery. A parity
harness must run both engines from the same cwd/tree.

### 6.7 Single-file validate

`validate_relationships_file(path)` → `_parsed_items([path])`, index has only
this file → cross-file refs all not-found (or self). `directory = path`;
`repository_root(path)` resolves the file's path and walks up.

---

## 7. Validation renderers

### 7.1 Human (`render_relationship_validation_human`, human.py 668)

```
Relationship Validation
<blank>
Relationships Checked: {relationships_checked}
Validation Issues: {validation_issues}
```
Then the issues are **partitioned by code** (not by emission order) into four
buckets and rendered in this fixed section order, each only if non-empty:

1. `duplicates` (`code == ISSUE_DUPLICATE_IDENTIFIER`):
   ```
   <blank>
   Duplicate Identifiers
   ✗ {identifier} ({len(paths)} files)      # red if tty
     - {p}                                   # each path in issue.paths
   ```
2. `unsupported` (`code == ISSUE_EDGE_UNSUPPORTED`), grouped by source path
   (change-detected on `issue.source_path`):
   ```
   <blank>
   Unsupported Relationships
   <blank>
   {source_path or "<input>"}
     ✗ {Label} not supported for this artifact type    # red if tty
   ```
   `Label = _relationship_label(issue.relationship)` (title-cased).
3. `cycles` (`code == ISSUE_RELATIONSHIP_CYCLE`):
   ```
   <blank>
   Relationship Cycles
   ✗ {Label} cycle:                          # red if tty
     - {p}                                    # each path in issue.paths
   ```
4. `references` (everything else — not-found/ambiguous/self/superseded/
   type-mismatch/scope), grouped by source then section:
   ```
   <blank>
   Broken Relationships
   <blank>
   {source_path or "<input>"}
     {Label}:                                 # printed when section changes
     ✗ {target} {suffix}                       # red if tty
   ```
   `suffix = _REF_ISSUE_SUFFIX.get(code, code)`:
   ```
   relationship-target-not-found   -> "not found"
   relationship-target-ambiguous   -> "ambiguous"
   relationship-self-reference     -> "self-reference"
   relationship-target-superseded  -> "superseded"
   relationship-target-type-mismatch -> "wrong target type"
   applies-to-target-not-found     -> "path not found"
   ```
   Any unmapped code falls back to the raw code string.

Grouping uses running `current_source`/`current_section` compared against
each issue in list order; because the emission order (§6.3) keeps a source's
reference issues contiguous and section-ordered, the grouping produces one header
per source/section run. `<input>` substitutes a None source_path (never happens
for these codes in practice).

`✗` = U+2717 (bytes `E2 9C 97`). Wrapped by `_red` (`\033[31m…\033[0m`) only on a
TTY. `_bold` for section headers, TTY only.

### 7.2 JSON (`render_relationship_validation_json`, json.py 260)

```json
{
  "directory": <raw arg>,
  "recursive": <bool>,
  "relationships_checked": <int>,
  "validation_issues": <int>,
  "issues": [ <issue.to_dict()>, ... ]     // in emission order (§6.3), NOT partitioned
}
```

`RelationshipIssue.to_dict()` emits **code-specific key sets** (key order matters):
- `duplicate-artifact-identifier`: `{"identifier", "paths", "code"}`.
- `relationship-edge-unsupported`: `{"source_path", "relationship", "code"}`
  (no `target`).
- `relationship-cycle`: `{"relationship", "paths", "code"}` (no `source_path`,
  no `target`).
- everything else (reference + scope): `{"source_path", "relationship", "target", "code"}`.

`indent=2`, `ensure_ascii=True` (escape non-ASCII), as §5.2.

### 7.3 SARIF (`render_relationships_sarif`, sarif.py 147)

One SARIF 2.1.0 document. `json.dumps(document, indent=2)` (ensure_ascii=True).

Per issue → `_relationship_result` (sarif.py 121). `label = (relationship or
"").replace("_", " ")` — **lowercase, spaces, NOT title-cased** (differs from
human). Message + uri per code:
- duplicate: `message = f"Duplicate artifact identifier '{identifier}' in: {', '.join(paths)}"`;
  `uri = paths[0]` (or identifier if empty).
- cycle: `message = f"{label} relationship cycle: {' -> '.join(paths)}"`; `uri = paths[0]`.
- edge-unsupported: `message = f"{label} not supported for this artifact type"`;
  `uri = source_path`.
- else: `reason = _RELATIONSHIP_REASON.get(code, code)`;
  `message = f"{label}: {target} — {reason}"`; `uri = source_path`.
  Reasons: not-found→`target not found`, ambiguous→`target is ambiguous`,
  self→`self-reference`, superseded→`target is superseded`,
  type-mismatch→`target is the wrong artifact type`,
  scope→`declared path does not exist in the repository`. The `—` here is U+2014,
  which `ensure_ascii` escapes to `—` in the JSON string.

`_result(rule_id, level, message, uri, line=None)`:
- `level = _LEVEL.get(RELATIONSHIP_SEVERITY.get(code, "warning"), "warning")`
  where `_LEVEL = {"error":"error","warning":"warning","info":"note"}`.
- `uri` is `urllib.parse.quote(uri, safe="/")` — **percent-encode** the path
  except `/`. Spaces → `%20`, non-ASCII → percent-encoded UTF-8 bytes. Landmine:
  Rust must replicate `quote(safe="/")` (RFC 3986 unreserved `A-Za-z0-9_.-~` +
  `/` stay literal; everything else percent-encoded).
- Result shape:
  `{"ruleId", "level", "message":{"text":…}, "locations":[{"physicalLocation":{"artifactLocation":{"uri":…}}}]}`
  (no `region` since line is None).

`_document(results)`: **sort** results by
`(uri, region.startLine or 0, ruleId, message.text)`; `rules =
[{"id": c} for c in sorted(set(ruleIds))]`. Document:
```json
{"version":"2.1.0","$schema":"https://json.schemastore.org/sarif-2.1.0.json",
 "runs":[{"tool":{"driver":{"name":"rac",
   "informationUri":"https://github.com/itsthelore/asdecided-core",
   "version":<__version__>,"rules":[…]}},"results":[…]}]}
```

**LANDMINE:** `version` = `rac.__version__`, a build-derived string (e.g.
`"0.1.dev50+g21c8be403"` from setuptools-scm) — **non-deterministic across
checkouts**. The parity harness must pin/normalize this field, or the Rust port
must emit a matching version string, for SARIF byte parity.

---

## 8. Parity landmine summary

1. **JSON `ensure_ascii=True`** — all non-ASCII escaped `\uXXXX` (astral →
   surrogate pair) in every `--json` and `--sarif` output. serde_json does the
   opposite by default. Verified: `café-ADR`.
2. **`str.casefold()`** everywhere for reference matching / dedup / index keys —
   full Unicode case folding, NOT `.lower()`. Rust needs real casefold.
3. **`str.splitlines()` + `str.strip()`** in `parse_references` and identity —
   Python's broad line-boundary set (`\v \f \x1c-\x1e \x85    `) and
   whitespace set (`\x1c-\x1f \x85 \xa0 …`, excludes `​`) diverge from Rust
   `lines()`/`trim()`. Reimplement both exactly.
4. **Per-artifact dict/edge order is `spec.optional` order (per-type), while
   `counts` is canonical `RELATIONSHIP_SECTIONS` order** — two different orderings
   in the same output; the unsupported/scope/reference emission orders each pick a
   specific one (see §6.3). Getting either wrong reorders JSON keys and lines.
5. **SARIF `version` (build-derived) + `quote(safe="/")` URI encoding + result
   sort key** — non-deterministic version and RFC-3986 percent-encoding must be
   matched/normalized; results sorted `(uri, line=0, ruleId, message)`.

Runner-up landmines worth guarding: TTY-gated ANSI color (`sys.stdout.isatty()`
→ no escapes when piped); `str.title()` label casing; em-dash `—`/middle-dot `·`
raw-UTF-8 in human labels; scope checks touch the real working tree via
`repository_root` (`.decided/config.yaml` discovery); frontmatter `id` strictness
gates whether references resolve at all.

## 9. Open questions / unverified

- **UNVERIFIED**: exact accepted grammar for frontmatter `id` (why `id: ADR-001`
  gave `metadata.id=None`) — owned by the frontmatter brief; resolution behavior
  depends on it.
- **UNVERIFIED**: prompt/design `spec.optional` exact order (read artifacts.py
  ~343/~434); only requirement/decision/roadmap enumerated here.
- **UNVERIFIED**: full byte behavior of `pathlib.Path` string normalization for
  odd args (`./x`, `x//y`, `x/./y`) feeding the `directory` field vs member paths
  — cross-check with the FS/corpus brief.
- **UNVERIFIED**: whether `roadmap` `retired_status` differs from
  `("Superseded","Deprecated")` (ADR-061 adds an "Achieved" terminal status) —
  affects `relationship-target-superseded` for roadmap targets; read artifacts.py
  roadmap spec.
- The `--recursive` flag is a documented no-op; confirmed by code
  (`recursive = not args.top_level`), but not exercised for a both-flags-set case
  beyond code reading.
