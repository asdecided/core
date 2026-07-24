# 09 — Corpus walk, stats, export, review, schema

Scope: the deterministic corpus walk (`rac.core.fs` + `rac.core.corpus`), and the
`stats`, `export`, `review`, `schema`/`templates` commands. Every claim below was
verified against the oracle (`.venv-oracle/bin/rac`, Python 3.11.15) unless marked
`UNVERIFIED`. Source files: `src/asdecided/core/fs.py`, `src/asdecided/core/corpus.py`,
`src/asdecided/services/stats.py`, `src/asdecided/services/export.py`,
`src/asdecided/services/review.py`, `src/asdecided/core/schema.py`, `src/asdecided/output/{human,json,sarif,templates}.py`.

The producing oracle version string is `0.1.dev50+g21c8be403` (setuptools_scm). This
string appears verbatim in two payloads (`export --json` `rac_version`, and every SARIF
`driver.version`) — see §3.7. It is environment-derived; a Rust port must be able to
inject/override it to match a given oracle build.

---

## 0. Cross-cutting output conventions (apply to ALL commands here)

### 0.1 Trailing newline
Every handler builds a string and emits it with a single Python `print()`, which appends
exactly one `\n`. So `stdout == render() + "\n"`. Exception chains noted per-command
(e.g. `schema --template` yields a body already ending in `\n`, so `print` makes it end
`\n\n` — see §5.4).

### 0.2 Human output color is TTY-gated (and computed ONCE at import)
`src/asdecided/output/human.py`:
```python
_USE_COLOR = sys.stdout.isatty()
def _c(text, code): return text if not _USE_COLOR else f"\033[{code}m{text}\033[0m"
```
- `_bold`→code `1`, `_green`→`32`, `_red`→`31`, `_yellow`→`33`.
- When stdout is NOT a TTY (pipe, file, capture) → **no ANSI bytes at all**. Verified: piped
  human output contains zero `\033`.
- `NO_COLOR` / `FORCE_COLOR` / `TERM` are **not** consulted. Only `isatty()`.
- Evaluated at module import time, so it is constant for the process.
- **Parity guidance**: golden/parity capture is always through a pipe → produce plain text.
  A Rust port should gate color on `stdout.is_terminal()` and otherwise emit no escapes.
  JSON/SARIF renderers never emit color regardless.

### 0.3 JSON serialization (`json.dumps(..., indent=2)`)
All JSON here uses `json.dumps(payload, indent=2)` with Python defaults EXCEPT
`export --documents` (JSONL, `ensure_ascii=False`, no indent — §3.6). Defaults imply:
- 2-space indent per level.
- Item separator `,\n` (comma then newline+indent); **no trailing whitespace on any line**.
- Key/value separator `": "` (colon-space).
- **`ensure_ascii=True`**: every non-ASCII scalar is escaped `\uXXXX` (lowercase hex). E.g.
  em-dash `—` → `—`. Astral chars → surrogate pair `😀`. This is the single
  most common JSON divergence — `serde_json` defaults to raw UTF-8. A Rust port MUST
  post-process or configure an ASCII-escaping serializer for parity on all JSON except the
  `--documents` JSONL stream.
- Empty containers: `[]` and `{}` render inline (no newline inside) even under `indent=2`.
  Nested: `"a": [],` on its own line. Verified.
- Key order = insertion order of the Python dict literal (NOT sorted). Preserve field order
  exactly as written in the code, per payload (spelled out below).
- Floats render via Python `repr(float)` (shortest round-trip). See §0.4.

### 0.4 Number formatting & Python `round()` (LANDMINE)
Several payloads round floats: `round(confidence, 2)`, `round(average, 1)`,
`round(coverage, 4)` (coverage is portfolio-owned; passed through by review). Python 3
`round()` is **round-half-to-even applied to the true binary double**, then serialized with
shortest-repr. Observed oracle values:
```
round(0.125,2)=0.12   round(0.375,2)=0.38   round(2.675,2)=2.67
round(5.35,1)=5.3     round(0.05,1)=0.1
json.dumps(0.8526)="0.8526"   json.dumps(5.3)="5.3"
```
`2.675→2.67` because `2.675` is actually `2.67499999…` in binary. A Rust port must replicate
CPython's correctly-rounded round-half-even (NOT naive `(x*100).round()/100`) AND emit the
shortest round-tripping decimal (Rust `{}`/`ryu` matches Python `repr` for f64 in the common
case; verify edge cases). Integers render as bare integers.

### 0.5 Human float formatting via `:.Nf` differs from JSON `round`
`stats` human prints the average with Python format `f"{x:.1f}"` (fixed 1 decimal, always
shows the digit, e.g. `0.0`), while `stats --json` uses `round(x,1)` then repr. Both use
round-half-even but the *string* forms differ (`0.0` vs `0.0`; but e.g. `10.0` in human vs
`10.0` JSON — align independently). Keep them as two distinct code paths.

---

## 1. The corpus walk — `find_markdown_files` (`src/asdecided/core/fs.py`)

This is THE traversal every command in this section (and most others) uses. Get it
byte-exact or every downstream ordering diverges.

```python
def find_markdown_files(directory: str, recursive: bool = True) -> list[Path]:
    root = Path(directory)
    glob = root.rglob if recursive else root.glob
    found = [
        p for p in glob("*.md")
        if not any(part.startswith(".") for part in p.relative_to(root).parts)
    ]
    return sorted(found)
```

### 1.1 Extension filter
- Pattern is the literal glob `*.md`. On Linux this is **case-sensitive**: `upper.MD`,
  `x.Md`, `x.markdown` are NOT matched. Only files whose name ends `.md` (lowercase).
  Verified: `upper.MD` excluded. A bare file named `.md` would be excluded by the hidden
  rule (§1.3) anyway.
- Only regular files/symlinks matching the glob; directories named `*.md` — `rglob("*.md")`
  would also yield a directory named e.g. `foo.md`. UNVERIFIED whether a dir named `*.md`
  occurs in practice; downstream `parse_file(str(path))` would then try to read a directory.
  Treat as out-of-corpus edge case; a port matching `rglob` semantics (which yields dirs too)
  is the safe choice, but corpora don't contain such dirs.

### 1.2 Recursion
- `recursive=True` (default) → `root.rglob("*.md")` (all descendants).
- `recursive=False` (CLI `--top-level` where offered) → `root.glob("*.md")` (direct children
  only).
- `stats`/`export` here always walk recursively (no top-level flag on `stats`/`export`).
  `review` honors `--top-level` (scope_parent) → passes `recursive=not args.top_level`.

### 1.3 Hidden exclusion (dirs AND files)
The filter drops any path where **any component of the path relative to root** starts with
`.`. This excludes:
- Hidden directories at any depth: `.git/`, `.venv/`, `.decided/`, `.hidden/` → all their `.md`
  files dropped.
- Hidden files: `.dotfile.md`, `.foo.md` dropped.
- Note it is `part.startswith(".")` on each **relative** part, so `root` itself being inside a
  dotted path (e.g. running against `/home/.config/x`) does NOT exclude — only components
  *below* `root` are checked (`p.relative_to(root).parts`). Verified: `.hidden/h.md` and
  `.dotfile.md` excluded; normal files kept.
- `..` never appears (rglob yields descendants only).

### 1.4 Symlinks (LANDMINE — asymmetric)
Python 3.11 `Path.rglob` semantics (verified empirically):
- A **symlinked FILE** matching `*.md` (e.g. `alink.md -> a.md`) **IS included**.
- A **symlinked DIRECTORY** is **NOT descended into**: `linkdir -> realdir/` with
  `realdir/r.md` present → `linkdir/r.md` does NOT appear, but the real `realdir/r.md` does.
- **Parity guidance**: Rust `walkdir`/`std::fs` defaults differ. Use a recursive walk that
  yields symlinked files but does NOT follow symlinked directories (do not set
  `follow_links(true)` blanket). Match Python 3.11 exactly. (Python ≥3.13 changed rglob
  symlink handling; the oracle is 3.11 — pin to 3.11 behavior.)

### 1.5 Sort order (LANDMINE — component-wise, NOT whole-string)
`sorted(found)` sorts `pathlib.Path` objects. Python 3.11 `PurePath.__lt__` compares
`self._cparts` — the **tuple of path components** — NOT the joined string:
```python
def __lt__(self, other): ... return self._cparts < other._cparts
```
Consequences (all verified):
- `"walktest/sub/c.md"` sorts **before** `"walktest/sub-x.md"`, even though as whole strings
  `"…/sub-x.md" < "…/sub/c.md"` (because `-`=0x2D < `/`=0x2F). Component-wise: parts
  `('walktest','sub','c.md')` vs `('walktest','sub-x.md')` → at index 1, `"sub" < "sub-x.md"`
  (shorter prefix wins). So the directory `sub/` sorts before sibling file `sub-x.md`.
- Within one directory level, filenames compare by **Python string ordering = Unicode code
  point**, case-sensitive: `Z.md`(0x5A) < `a.md`(0x61). Uppercase before lowercase.
- Empirical full ordering of a mixed fixture:
  ```
  Z.md, a.md, alink.md, b.md, realdir/r.md, space file.md,
  sub/c.md, sub-x.md, sub.md, sub0.md
  ```
  Note `sub/c.md` (dir) < `sub-x.md` < `sub.md` < `sub0.md`, and among the trailing three:
  `-`(0x2D) < `.`(0x2E) < `0`(0x30).
- **Parity guidance**: sort by the sequence of path components, comparing each component by
  Unicode scalar value, with the shorter-is-prefix rule. Rust `std::path::Path`'s `Ord`
  compares components (`Components`) and matches this for ASCII/UTF-8 (UTF-8 byte order equals
  code-point order). Do NOT sort by the joined path string — it diverges at the `/` boundary.

### 1.6 Path string form emitted downstream
Every `path`/`file` field downstream is `str(path)` where `path = root / <relative>` from
rglob. `root = Path(directory_arg)`, which **normalizes**:
- `Path("rac/")` → `"rac"` (trailing slash stripped); emitted paths → `rac/decisions/x.md`.
- `Path("./rac/")` → `"rac"` (leading `./` stripped).
- `Path("rac//")` → `"rac"`; `Path("rac/./x")` → `"rac/x"`.
- Absolute stays absolute.
So the `directory` **argument** you pass changes the `path` prefix in output. The `directory`
FIELD in payloads is the raw arg string (unmodified), but per-artifact `path` fields are
normalized+prefixed. A port must apply the same `PurePosixPath` normalization to the root arg
before joining.

### 1.7 The walk → parse → classify seam (`corpus.py`)
`walk_corpus(directory, recursive=True)` = for each path from `find_markdown_files`, in that
sorted order: `parse_file(str(path))` then `classify(product)`, yielding
`CorpusEntry(path, product, classification)`. Lazy; ordering identical to §1.5. `collect_corpus`
is the eager list form (adds progress/cancel hooks that don't affect bytes). Parse errors do
NOT abort the walk — `parse_file` degrades a bad file to a product carrying an
`unreadable-artifact` parse issue (that classifies as `unknown`). `artifact_type` property =
`classification.type` (`"unknown"` is a valid classified outcome).

### 1.8 Cache does NOT touch these commands
`CorpusCache` / `corpus_content_hash` (ADR-099/106/112, "warm by default") is only wired into
`validate` (directory), `relationships`, `doctor`, `gate`, `index`, `mcp`, `find` — the
subcommands that register `--cache/--no-cache`. **`stats`, `export`, `review`, `schema`,
`templates` have NO cache flag** (parents are only `version_parent`/`json_parent`/etc). They
call `collect_stats` / `build_*_export` / `build_review` which invoke `walk_corpus` fresh every
time. Therefore:
- The cache can **never** change the output bytes of stats/export/review/schema, and there is
  no `--no-cache` to disable on them.
- Even where the cache IS used (validate etc.), the design contract is byte-identical output:
  `CorpusCache` only short-circuits reparse of byte-unchanged files, and identical bytes reparse
  to an identical `Product` (docstring REQ-003). The flag to disable globally is env
  `DECIDED_NO_CACHE=<nonempty>`; per-invocation `--no-cache`. `_cache_enabled(args) = args.cache and
  not os.environ.get("DECIDED_NO_CACHE")`. For THIS section, treat the cache as irrelevant to output.

---

## 2. `rac stats <directory>` (`services/stats.py`, `output/{human,json}.py`)

CLI: `p_stats` parents `[version_parent, json_parent]`. One positional `directory`. Flags:
`--json` only (plus `--version`). No `--top-level`, no cache. Always recursive.

Guard: `if not Path(args.directory).is_dir(): _usage_error("not a directory: <dir>")` →
prints `rac: error: not a directory: <dir>` to stderr, exit 2 (usage). (See §6 exit codes.)

### 2.1 Aggregation (`collect_stats`)
Walks `walk_corpus(directory)` (recursive). For each entry: `name = product.title or path.stem`
(the title, else filename stem without `.md`). Runs `build_inspection(product)` → `result.type`.
Routing by classified type (each branch `continue`s; order of checks matters only for exclusivity):
- **relationship presence** (tallied for EVERY recognized type before routing): for each present
  relationship section (`present_relationship_sections`) increment `rel_counts[section]`.
- `decision` → `DecisionStat(path, name, status, category, supersedes)` (from inspection).
- `roadmap`/`prompt`/`design` → lightweight `_validity_stat` (valid iff no error-severity issues;
  `error_codes` = list of `issue.code` where `severity=="error"`).
- `unknown` → `UnrecognizedStat(path, name, confidence)`.
- else (requirement / recognized feature type) → `FeatureStat(path, name, valid, error_codes,
  requirements=len(product.requirements), success_metrics=len(product.success_metrics),
  risks=len(product.risks))`.
Finally `relationship_counts` re-ordered into canonical `RELATIONSHIP_SECTIONS` order (only keys
present kept).

### 2.2 Derived quantities / orderings
- `files_found` = number of FeatureStats (requirement features only — decisions/roadmaps/etc are
  separate).
- `missing_metrics` / `missing_risks` = feature NAMES with 0 metrics / 0 risks, **in walk order**
  (list comprehension over `self.features`, which are appended in sorted-path walk order).
- `average_requirements` = `total_requirements / files_found` (float), or `0.0` if no features.
- `largest_feature` = `max(features, key=(f.requirements, _neg_name(f.name)))`. Tie-break:
  `_neg_name(name) = tuple(-ord(c) for c in name)` → makes the **alphabetically-earliest** name
  win a requirements tie (because larger negated tuple = smaller name). `None` if no features.
- `requirements_by_feature` = `sorted(features, key=lambda f: (-f.requirements, f.name))` →
  requirement count DESC, then name ASC (plain Python string order on the name).
- `invalid` = features with `valid==False`, in walk order.
- Decision buckets `_bucket(...)`: count by `status`/`category`; ordering = the decision spec's
  declared metadata order first (`status`: Proposed, Accepted, Superseded, Deprecated;
  `category`: Architecture, Product, Process, Technical, Other), **omitting empty buckets**, then
  any out-of-vocabulary values seen, appended in `sorted()` (code-point) order.

### 2.3 JSON output (`render_stats_json`) — exact key order
Top-level dict, keys in THIS order (always present):
```
directory, empty, features, valid_features, invalid_features, requirements, metrics, risks,
features_missing_metrics, features_missing_risks, missing_metrics, missing_risks,
average_requirements_per_feature, largest_feature, requirements_by_feature, invalid
```
- `directory` = raw arg string.
- `empty` = `is_empty` (true only when zero recognized AND zero unrecognized artifacts).
- `average_requirements_per_feature` = `round(average, 1)`.
- `largest_feature` = `{"name","requirements"}` or `null`.
- `requirements_by_feature` = list of `{"name","requirements"}` in the DESC/ASC order above.
- `invalid` = list of `{"file": path, "errors": [codes]}` (walk order).
Then **conditionally appended** (only when the corresponding list is non-empty), in this order:
- `decisions` → `{"count", "by_status", "by_category"}` (buckets per §2.2).
- `roadmaps` → `{"count", "valid", "invalid":[{"file","errors"}]}`.
- `prompts` → `{"count", "valid", "invalid":[…]}`.
- `designs` → `{"count", "valid", "invalid":[…]}`.
- `unrecognized` → `{"count", "files":[{"file","name","confidence": round(conf,2)}]}` (walk order).
- `relationships` → `{ <section with spaces→underscores>: count, ... }` in `RELATIONSHIP_SECTIONS`
  order. NOTE: keys here replace `" "` with `"_"` (e.g. a section "related decisions" →
  `related_decisions`). These are declared-presence counts, not edge counts.
Non-ASCII in names is `\uXXXX`-escaped (§0.3): e.g. `RAC v0.5.0 — Artifact Improvement`.
Exit code: see §6.

### 2.4 Human output (`render_stats_human`) — exact structure
Blocks joined by `\n` (bold markers only if TTY):
```
Portfolio Overview
==================
<blank>
Features: {files_found}
Requirements: {total_requirements}
Metrics: {total_metrics}
Risks: {total_risks}
<blank>
Quality
=======
<blank>
Features Missing Metrics: {n}
  - {name}            (one per missing-metric feature, walk order)
Features Missing Risks: {n}
  - {name}
Average Requirements Per Feature: {avg:.1f}
Largest Feature: {name} ({k} requirements)   OR   Largest Feature: (none)
<blank>
Requirements by Feature
=======================
<blank>
{name padded to (maxNameLen+4)}{requirements}   (per feature, DESC/ASC)   OR   (none)
```
Then conditional sections (each omitted entirely when empty), in this exact order:
- `Invalid Features ({n})` header + lines `  {red(path)} — {codes joined ", " or "unknown"}`.
  Header line is `_bold("Invalid Features (N)")`; note the em-dash ` — ` (U+2014, raw UTF-8 in
  human output).
- `Decisions` block: `=========`, blank, `Total: {n}`, then `Status`/`Category` breakdowns; each
  breakdown a blank + bold label then `  - {name}: {count}` lines, or `  (none recorded)`.
- `Roadmaps` (`========`): blank, `Total`, `Valid`, then optional `Invalid Roadmaps (n)` list.
- `Prompts` (`=======`): same shape.
- `Designs` (`=======`): same shape.
- `Unrecognized` (`============`): blank, `{count} document|documents matched no known artifact
  schema (not errors — see ADR-010):`, then `  {path}` per doc. (`document` if count==1 else
  `documents`.)
- `Relationships` (`=============`): blank, then `Artifacts with {section.title()}: {count}` per
  section (`.title()` = Python title-case of the section name).
- If `is_empty`: append blank + `No artifacts yet — create your first with: rac quickstart`.
The underline strings (`==================`, etc.) are literal fixed-length underlines baked into
the code — reproduce the exact character counts shown.
Column width in "Requirements by Feature": `width = max(len(name) for features) + 4`; each line is
`f"{name:<{width}}{requirements}"` (left-justified pad with spaces). Verified plain-text (no color)
when piped.

---

## 3. `rac export [directory]` (`services/export.py`, `output/{json}.py`)

CLI: `p_export` parent `[version_parent]`. Positional `directory` (`nargs="?"`, default `"."`).
Mutually-exclusive write group: `--html`, `--okf`, `--documents`, `--graph`, `--agent-rules`.
`--json` is separate (NOT in the group). Also `--check`, `--client`, `--out`.
Guard: `not is_dir` → usage error exit 2.

Determinism contract (module docstring): NO timestamps, NO env-dependent fields except
`rac_version`; artifacts in sorted-path order; relationships sorted. Two exports of the same tree
(same rac_version) are byte-identical. **`export --graph` and `--documents` carry no version/time
field at all** — fully corpus-derived. Only `--json` (viewer payload) and `--okf` embed
`rac_version`.

### 3.1 Mode dispatch & exit codes (all EXIT_OK=0 on success)
- `--agent-rules` → separate handler (`_cmd_agent_rules`), out of this section's scope.
- `--check` without `--agent-rules` → usage error. `--client` without `--agent-rules` → usage error.
- `--json` with `--html`/`--okf` → usage error `--json cannot combine with --html or --okf`.
- `--out` without `--html`/`--okf` → usage error `--out requires --html or --okf (--json writes to
  stdout)`.
- `--documents` → print JSONL to stdout, exit 0.
- `--graph` → print graph JSON to stdout, exit 0.
- `--okf` → write bundle dir, print `wrote {out}/ — {n} artifact(s), {e} relationship(s)`, exit 0
  (OKF is another agent's scope; message line noted for completeness).
- default (no write mode, or `--json`): print viewer JSON to stdout, exit 0. `--json` is an
  explicit no-op here (JSON is always the default).
- `--html` → write file (Portal), separate scope.

### 3.2 Shared walk (`_walk_entries`)
One `walk_corpus(directory, recursive)` pass → list of `_WalkedEntry(entry, path=str(entry.path),
spec=spec_for(artifact_type))`. `spec is None` ⇔ type `unknown`. Order = §1.5 sorted-path.
`_canonical_by_path` maps EVERY entry's path (unknown included) → `artifact_identifier(product,
spec, path)` — this map is the resolution index for edges (so a ref resolves in export exactly
when relationship-validation resolves it).

### 3.3 Corpus/viewer JSON (`build_corpus_export` → `render_export_json`)
`CorpusExport.to_dict()` key order:
```
schema_version="1",
corpus: { name, rac_version, artifact_count },
artifacts: [ ExportArtifact.to_dict(), ... ],
relationships: [ ExportRelationship.to_dict(), ... ]
```
- `corpus.name` = `_corpus_name(directory)` = `PurePath(directory.rstrip("/")).name or directory`.
  Basename of the arg with trailing `/` stripped; NOT filesystem-resolved (so it never depends on
  cwd). E.g. `rac/` → `rac`; `.` → basename of `.` which is `""` so falls back to arg `"."`? —
  `PurePath(".").name == ""`, so `_corpus_name(".")` returns `"."` (the `or directory` fallback).
  Verified.
- `corpus.rac_version` = `rac.__version__` (setuptools_scm string; §0 header).
- `artifact_count` = number of exported artifacts (excludes unknown).
- **Artifacts**: iterate walk; SKIP `spec is None` (unknown not exported). Each `ExportArtifact.to_dict()`:
  ```
  id, aliases, type, status, title, path, body_html
  ```
  (`tags` is on the dataclass but DELIBERATELY excluded from `to_dict` — do not emit it.)
  - `id` = canonical identifier (`canonical_by_path[path]`).
  - `aliases` = `artifact_identifiers(product, spec, path)` (list; identity agent owns exact rules).
  - `type` = classified artifact type.
  - `status` = `_status`: first non-empty of the `## Status` section body, canonicalized against the
    type's declared status values exactly as `rac inspect` (`canonical_value`), e.g. `accepted` →
    `Accepted`. **Absent/empty status → the literal string `"unknown"`** (`STATUS_ABSENT`). Always a
    string (never null).
  - `title` = `product.title or canonical` (untitled falls back to the canonical id — NOT null,
    unlike `rac index`).
  - `path` = `str(path)` (normalized+prefixed per §1.6).
  - `body_html` = the on-disk Markdown body **after the frontmatter envelope** (re-read from disk via
    `split_frontmatter`, NOT the parsed/normalized body), rendered by `markdown-it-py` with preset
    `"commonmark"` and `{"html": False}` (raw HTML **disabled** → source HTML is escaped, not
    executed). Body rendering fidelity is the markdown agent's contract; note the raw-HTML-off flag
    and the raw-bytes-after-frontmatter source here.
- **Relationships**: `relationships_from_corpus([entries])` → for each rel, `ExportRelationship(
  from_=canonical_by_path[rel.source_path], to=canonical_by_path[rel.resolved_path] if resolved else
  rel.target)`. Then `edges.sort(key=lambda e: (e.from_, e.to))` — sorted by `(from, to)` string
  tuples (code-point order). `.to_dict()` → `{"from", "to", "type"}` where `type` is the constant
  `"relates-to"` (viewer flattens all edge kinds to this). Unresolved target = literal reference text
  preserved verbatim.

### 3.4 Graph JSON (`build_graph_export` → `render_graph_json`) — ADR-074
`GraphExport.to_dict()` key order:
```
schema_version="1", source=<corpus_name>, nodes:[...], edges:[...]
```
Note the top key is **`source`** (not `corpus`), and there is NO `rac_version`/timestamp.
- **Nodes**: walk order, SKIP unknown. `GraphNode.to_dict()` → `{"id","type","status","title"}`
  (same id/status/title derivation as §3.3; no aliases/body/path). Sorted-path order (walk order).
- **Edges**: for each `rel` in `relationships_from_corpus`: `kind = edge_spec(rel.relationship)`.
  `GraphEdge.to_dict()` key order:
  ```
  source, target, type, directed, resolved, external, provider
  ```
  - `source` = `canonical_by_path[rel.source_path]`.
  - `target` = canonical of `resolved_path` if resolved else literal `rel.target`.
  - `type` = `rel.relationship` (the REGISTRY edge kind — `supersedes`, `related_decisions`, …,
    NOT flattened to `relates-to`).
  - `directed` = `kind.directional if kind else False`.
  - `resolved` = `rel.resolved_path is not None` (bool).
  - `external` = `kind.external if kind else False` (external ticket refs, ADR-087).
  - `provider` = configured ticketing provider (`load_ticketing_provider(directory)`, read once)
    **only when** `kind and kind.external_provider`, else `null`.
  - Sort: `edges.sort(key=lambda e: (e.source, e.type, e.target))` — by `(source, type, target)`
    string tuples (code-point order). This differs from the viewer edge sort `(from, to)` — note
    the extra `type` in the middle of the key.

### 3.5 Nodes vs artifacts parity
`build_graph_export` nodes and `build_corpus_export` artifacts are the SAME set (classified only),
SAME sorted-path order. Only the projected fields differ.

### 3.6 Documents JSONL (`build_documents_export` → `render_documents_jsonl`)
- Output is **JSON Lines**: one compact `json.dumps(record, ensure_ascii=False)` per record, joined
  by `\n` (then the trailing `print` newline). **`ensure_ascii=False`** → UTF-8 body emitted raw
  (the ONE JSON surface here that does not `\uXXXX`-escape). Compact = default separators
  `(', ', ': ')` (default when `indent=None` and no `separators=` given). Verified:
  `json.dumps({'a':1,'b':[1,2]}, ensure_ascii=False)` → `{"a": 1, "b": [1, 2]}` — item sep is
  `", "` (comma-SPACE), key sep `": "`. serde_json compact uses `,`/`:` with NO spaces, so a Rust
  port MUST emit the space-after-comma / space-after-colon compact form for this JSONL. Zero records
  → empty string → single trailing newline (verified: an all-unknown dir yields one blank line).
- Records SKIP unknown. Walk (sorted-path) order. `ExportDocument.to_dict(source=corpus_name)` key
  order:
  ```
  schema_version="1", id, type, status, title, text,
  metadata: { path, aliases, tags, source }
  ```
  - `text` = the Markdown **body** (frontmatter stripped), NOT HTML.
  - `metadata.source` = corpus name (namespacing).
  - `tags` IS emitted here (unlike the viewer artifact).

### 3.7 Version/time-derived fields summary
| surface | version field | timestamp? |
|---|---|---|
| `export --json` | `corpus.rac_version` = `rac.__version__` | none |
| `export --okf` | embeds rac_version + git recency (log.md) | git-derived |
| `export --graph` | NONE | none |
| `export --documents` | NONE | none |
| all SARIF | `driver.version` = `__version__` | none |
`rac_version` is the only env-derived field in the graph/json/documents family and appears ONLY in
`--json`. Rust port must inject the matching version string to compare `--json` output.

---

## 4. `rac review <directory>` (`services/review.py`, `output/{human,json,sarif}.py`)

CLI: `p_review` parents `[version_parent, json_parent, scope_parent]`. Positional `directory`.
Flags: `--json`, `--sarif`, `--top-level` (from scope_parent → `recursive=not top_level`),
`--stale-after [DAYS]` (`nargs="?"`, `type=int`, `const=14`, `default=None`).
Guards: `not is_dir` → usage error exit 2; `--stale-after < 0` → usage error
`--stale-after must be a non-negative number of days`.

### 4.1 GIT-DERIVED / TIME-DERIVED output (LANDMINE)
`build_review` output is **NOT a pure function of file bytes**. It composes:
1. `portfolio_from_corpus` (deterministic, byte-derived — portfolio agent's scope; but its
   `relationships.coverage` is `round(x,4)` and `health.score` are portfolio-computed).
2. `_drift_findings` → `suspect_drift(directory, entries)` — a **git-native** advisory
   (`priority=6`, `PRIORITY_SUSPECT_DRIFT`). Reads `git log` commit dates. Emits one
   `REVIEW_SUSPECT_ARTIFACT` ("suspect-artifact") finding per referrer whose resolved target
   changed more recently in git history. **Outside a git repo, or with no git binary, this is
   empty.** On the live `rac/` corpus this produced **34 priority-6 findings** — so review output
   depends on the git checkout state.
3. `--stale-after` → `_cadence_finding` uses `recency_from_corpus` (git `git log`) AND
   `datetime.now(UTC)` (WALL CLOCK) → `priority=5` `stale-corpus` finding when newest artifact is
   older than the window. Message embeds `age.days` (a live-clock value). Injectable `now` exists in
   the API but the CLI never passes it → real wall clock.
**Parity guidance**: to get deterministic review parity, the Rust port must replicate the git
derivation (commit-date comparison via `git log`) identically, OR the harness must run in a fixed
git state / without git (empty drift) and without `--stale-after`. Priority 1–4 findings ARE pure
byte-derived; priority 5–6 are the git/time contamination. Document any parity harness as running in
the same git state as the oracle.

### 4.2 Priority buckets & finding construction
Constants: 1 invalid-artifact, 2 broken-relationship, 3 unknown-artifact, 4 missing-recommended,
5 stale-corpus, 6 suspect-drift.
- Portfolio `attention` items re-ranked via `_ATTENTION_PRIORITY` (invalid→1, broken-rel→2,
  missing-recommended→4; unknown attention code → default 4). Each gets a deterministic `action`:
  - invalid → `Run: rac validate {path}`
  - broken-relationship → `Run: rac relationships {directory} --validate`
  - else → `Run: rac improve {path} --template`
  And `impact` from the Core `_IMPACT` map (unknown code → `"This finding affects repository
  quality."`).
- `portfolio.unknown_paths` → one `unknown-artifact` finding each, priority 3, severity `info`,
  `identifier = Path(path).stem`, message `"No artifact schema matched this document."`, action
  `Run: rac inspect {path} (see rac schema --list)`.
- Drift findings: severity `warning`, `identifier = Path(source_path).stem`, code
  `suspect-artifact`, `message = drift_problem(record)`, action `Run: rac doctor {directory}`.
- Cadence finding: severity `info`, path=`directory`, identifier=`corpus`, code `stale-corpus`,
  message `No product knowledge recorded in the last {window} days (newest artifact is {age.days}
  days old).`, action `Run: rac new decision rac/decisions/<name>.md`.

### 4.3 Finding ordering
Two-stage. `review_from_portfolio` first sorts its issues by `(priority, path, code)`. Then
`build_review` appends advisories (drift + optional cadence) and, if any advisory added, **re-sorts
the whole list** by `key=(i.priority, i.path, i.code)` (all string/int tuple, code-point order on
path/code). If NO advisories, the list keeps the `review_from_portfolio` sort (same key). Net:
final order is always `(priority ASC, path ASC, code ASC)`.

### 4.4 `ok` and exit code
`report.ok = not any(i.priority <= 2 for i in issues)` → True unless there is a priority-1 or -2
finding. Exit: `EXIT_OK (0)` if `ok` else `EXIT_VALIDATION_FAILED (1)`. Priority 3–6 are advisory
and never fail. Verified: live `rac/` review → ok=True, exit 0, with priorities {4:89, 6:34, 3:11}.

### 4.5 JSON (`render_review_json` → `ReviewReport.to_dict()`) — exact key order
```
schema_version="1", directory, recursive, ok, empty,
artifacts: { total, by_type, unknown_paths },
validation: { valid, invalid },
relationships: { total, valid, broken, orphaned, coverage },
health: { score },
issues: [ ReviewIssue.to_dict(), ... ],
actions: [ str, ... ]
```
- `empty` = `portfolio.total_artifacts == 0`.
- `by_type` = portfolio dict (ordered requirement, decision, roadmap, prompt, design, unknown —
  portfolio-owned order; verified live).
- `relationships.coverage` = portfolio value (`round(x,4)`; e.g. `0.8526`).
- `health.score` = portfolio int.
- `ReviewIssue.to_dict()` key order: `priority, severity, path, identifier, code, message, action,
  impact`.
- `actions` = deduplicated `issue.action` values in issue (priority) order — first occurrence
  wins, dedup via a seen-set.

### 4.6 Human (`render_review_human`)
Structure (bold only if TTY):
```
Repository Review
=================
<blank>
Directory:  {directory}
Artifacts:  {total_artifacts}
<blank>
  {Type.title():<14} {count}      (only types with count>0, portfolio by_type order)
<blank>
Validation
----------
<blank>
  Valid:    {valid}
  Invalid:  {invalid}
<blank>
Relationships
-------------
<blank>
  Total:    {n}
  Valid:    {n}
  Broken:   {n}
```
Then IF issues:
```
<blank>
Issues ({len})
------
```
then per priority in `_PRIORITY_LABELS` order (1..6) with a non-empty group:
```
<blank>
  Priority {p} — {label}:
    {icon} {identifier}
        {message}
```
`_PRIORITY_LABELS`: 1 "Invalid artifacts", 2 "Broken relationships", 3 "Unrecognized artifacts",
4 "Missing recommended information", 5 "Write cadence", 6 "Possible drift (review recommended)".
Icon: severity error→`_red("✗")`, warning→`_yellow("!")`, else `·` (middle dot U+00B7). Then:
```
<blank>
Suggested Actions
-----------------
<blank>
  {n}. {action}     (1-indexed, dedup order)
```
ELSE (no issues): `<blank>` + `_green("✓ Nothing needs attention.")`.
Always then:
```
<blank>
Health Score
------------
<blank>
  {score_color(str(score))} / 100
```
`score_color = _green if score>=80 else _yellow if score>=60 else _red`. If `total_artifacts==0`,
append `<blank>` + `No artifacts yet — create your first with: rac quickstart`.

### 4.7 SARIF (`render_review_sarif`, `output/sarif.py`)
- One result per issue: `_result(code, severity, message, path, line=None)`. Message =
  `f"{issue.message} — {issue.action}"` when action present, else just message (all review issues
  have actions).
- `_result`: `uri = quote(path, safe="/")` (percent-encode spaces/non-ASCII, keep `/`). No `region`
  (line None). `level` via `_LEVEL = {error:"error", warning:"warning", info:"note"}` (default
  "warning").
- `_document`: results sorted by `(uri, startLine|0, ruleId, message.text)`. `rules` =
  `[{"id": code} for code in sorted(set(ruleIds))]` (unique codes, sorted). Top document:
  `{version:"2.1.0", $schema:<schemastore url>, runs:[{tool.driver:{name:"rac",
  informationUri:"https://github.com/itsthelore/asdecided-core", version:__version__, rules}, results}]}`.
  `json.dumps(indent=2)` (ensure_ascii=True). `driver.version` = env version.

---

## 5. `rac schema` / `rac templates` (`core/schema.py`, `output/{human,json,templates}.py`)

### 5.1 `rac schema --list`
CLI: `p_schema` parent `[version_parent]`; positional `schema` (`nargs="?"`); `--list`; mutually
exclusive `--json`|`--template`. Handler `cmd_schema`:
- `--list` + `--template` → usage error `--template cannot be used with --list`.
- `--list` + positional name → usage error `schema name cannot be used with --list`.
- `--list` alone: emit list, exit 0.
  - JSON (`render_schema_list_json`): `{"schemas": names}` (indent=2). Names order =
    `available_schemas()` = `[spec.name for spec in ARTIFACT_SPECS]`. Verified order:
    `["requirement","decision","roadmap","prompt","design"]`.
  - Human (`render_schema_list_human`): `_bold("Available Schemas:")` then `- {name}` per name.

### 5.2 `rac schema <name>` (no `--list`)
- No name and no `--list` → usage error `schema name required unless --list is passed`.
- `ref = schema_reference(name)`; if unknown → print `render_unknown_schema(name, names)` to
  **stderr** and `raise SystemExit(2)` (usage). Body:
  `Unknown schema: {name}\n\nAvailable schemas:\n- {each}`. Verified exit 2.
- `--json` → `render_schema_json(ref)` = `ref.to_dict()` (indent=2). Key order:
  ```
  type, required, recommended, optional, descriptions, guidance, metadata
  ```
  Section-name lists are **snake_cased** via `_snake` (`" "`→`"_"`). `descriptions`/`guidance`/
  `metadata` keys also snake-cased. `guidance` values are lists of strings; `metadata` values are
  lists of allowed values in declared order. (Full decision example captured in recon — e.g.
  metadata.status = [Proposed, Accepted, Superseded, Deprecated].) `starter_bodies` is NOT emitted.
- `--template` → `render_schema_template(ref)` (see §5.4).
- else → `render_schema_human(ref)`.

### 5.3 Human schema (`render_schema_human`)
`_bold("Artifact Type: {display}")`, blank, then three blocks via `section_block`:
`Required Sections:`, `Recommended Sections:`, `Optional Sections:`. Each: bold title; if no
sections `  (none)` + blank; else per section `  - {name.title()}`, optional
`      Description: {desc}` (if present), optional `      Guidance:` + `        - {item}` lines;
trailing blank after the block. Then if metadata: `_bold("Metadata Fields:")` and
`  - {name.title()}: {values joined " | "}`. Whole thing `.rstrip()`-ed (trailing whitespace/newlines
stripped) before the `print` adds one `\n`. NOTE names here are `.title()`-cased (display form),
whereas JSON snake-cases them — different casing per surface.

### 5.4 Template (`render_schema_template`) — `output/templates.py`
Returns:
```
# Title
<blank>
## {Section.title()}
<blank>
{section.body}
<blank>              (only if comments)
<!-- Choose one: v1 | v2 | ... -->    (if metadata_values)
<!-- {guidance line} -->              (per guidance line)
```
- Sections = `template_sections(ref)` = `required + recommended` (optional EXCLUDED from template).
- `section.body` = `_starter_body`: if the section has metadata_values → `_metadata_default`
  (status→"Proposed" if allowed, category→"Other" if allowed, else `values[0]`); else
  `ref.starter_bodies.get(section)` or fallback `f"TODO: describe {section}."`.
- Blocks joined by `\n\n`; whole string ends with `+ "\n"`. Then `print` adds another `\n` →
  **final output ends with `\n\n`** (one from the function, one from print). Pin this.
- `--template` cannot combine with `--list` (see §5.1). Section titles use `.title()`.

### 5.5 `rac templates` (`cmd_templates`)
CLI parent `[version_parent, json_parent]`; `--json`.
- JSON (`render_templates_json`): `{"schema_version":"1","templates": names}` (indent=2). Note the
  extra `schema_version` key vs `schema --list --json` which has none. Names same order as §5.1.
- Human (`render_templates_human`): `_bold("Available artifact templates:")`, blank, then
  `- {name}` per name. (Differs from `schema --list` human by the blank line after the header and
  the header text.)
Verified both. Exit 0.

---

## 6. Exit codes (this section's commands)

Constants (from `cli.py`): `EXIT_OK=0`, `EXIT_VALIDATION_FAILED=1`, `EXIT_USAGE=2`.
`_usage_error(msg)` prints exactly `rac: {msg}\n` to **stderr** (NOT `rac: error:` — it is a
hand-written line, not argparse's `parser.error`) and `raise SystemExit(2)`. Verified:
`rac stats /nonexistent-xyz` → stderr `rac: not a directory: /nonexistent-xyz`, exit 2.
Exception: the unknown-schema case (§5.2) prints a multi-line blob built by
`render_unknown_schema` (no `rac:` prefix) to stderr, then exit 2.

| command | success (0) | 1 | 2 (usage) |
|---|---|---|---|
| `stats` | `has_meaningful_content OR is_empty` | else (files exist, none valid known) | not a directory |
| `export --json/--graph/--documents/--okf` | always on success | — | bad flag combo / not a dir |
| `review` | `report.ok` (no priority ≤2) | priority 1 or 2 present | not a dir / negative `--stale-after` |
| `schema --list` / `schema <name>` (human/json/template) | 0 | — | conflicting flags / missing name / unknown schema |
| `templates` | 0 | — | — |

`stats` empirical: live `rac/` exit 0. `review` live exit 0. Note the `stats` "no valid known
artifacts in a non-empty corpus" → exit 1 path (guarded so a day-one empty corpus is exit 0).

Broken-pipe note: piping any of these into `head` can surface a Python `BrokenPipeError` and a
non-zero exit (observed `1` when `head` closed the pipe early). This is NOT the command's real exit
status — measure exit codes without truncating the stream. A Rust port should handle SIGPIPE/EPIPE
gracefully (typically exit on SIGPIPE) but this is not a parity-relevant output-byte concern.

---

## 7. Verification status

Verified empirically (Python 3.11.15, oracle build `0.1.dev50+g21c8be403`): walk sort order &
symlink asymmetry & hidden/extension filters (§1); component-wise Path sort mechanism
(`_cparts`, §1.5); path normalization (§1.6); `stats`/`review`/`export --graph`/`export --json`
exit codes and JSON shape on live `rac/`; `schema --list`, `schema <name> --json`, `schema nope`
(exit 2), `templates`, `templates --json`; `ensure_ascii` escaping and float `round` behavior
(§0.3–0.4); no-color when piped (§0.2); no cache flags on these commands (§1.8).

Also verified after first draft: `--documents` JSONL compact separators `", "`/`": "` (§3.6);
`_corpus_name(".")` → `"."` (§3.3); usage-error stderr prefix is `rac: {msg}` (§6).

UNVERIFIED / delegated: the `parse`/`classify`/`inspect` internals and `portfolio` health/coverage
math (other agents); markdown-it body rendering fidelity (markdown agent); a directory literally
named `*.md` (§1.1); OKF bundle contents (OKF agent — only the stdout summary line noted here).
