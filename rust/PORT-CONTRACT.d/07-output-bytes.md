# 07 — Output Bytes: JSON / SARIF / Human / OKF / Portal formatting

Scope: everything under `src/asdecided/output/` plus every `json.dumps` call site in
`src/`. Goal: byte-for-byte reproduction of stdout and exit codes. All line
numbers are into the frozen oracle at `src/`.

Verified empirically with `.venv-oracle/bin/rac` unless marked UNVERIFIED.

---

## 1. `json.dumps` default semantics (the master rule)

Python `json.dumps` defaults that the Rust port MUST replicate exactly:

| Param | Default (unless overridden) | Effect |
| --- | --- | --- |
| `indent` | `None`, but nearly all RAC calls pass `indent=2` | 2-space nesting |
| `separators` | derived from `indent` | see below |
| `ensure_ascii` | `True` (default) — RAC overrides to `False` at specific sites only | non-ASCII → `\uXXXX` |
| `sort_keys` | `False` | keys emitted in **dict insertion order**, never sorted |
| `default` | none used anywhere | every value must be natively JSON-serialisable (str/int/float/bool/None/list/dict); a stray object would raise `TypeError` — the port must mirror the exact key set the Python dict builds |

### 1.1 Separators (CRITICAL — trailing-whitespace trap)

`json.dumps` chooses separators from `indent`:

- **`indent=None`** (compact): `separators=(", ", ": ")` — item sep is
  comma-**space**, key sep is colon-space.
- **`indent=2`** (or any int): `separators=(",", ": ")` — item sep is a **bare
  comma with NO trailing space** (the newline+indent follows it), key sep is
  `": "`.

So with `indent=2` a line ends `"key": value,` — comma, then `\n`, then the
next line's 2-space-per-level indent. There is **never** a space before a
newline. A naive pretty-printer that emits `", "` will mismatch every multi-key
object. The compact (`indent=None`) sites (`usage.py:72`, `budget.py:66`,
`derived_cache.py:543`, `documents_jsonl`) DO use `", "` and `": "`.

### 1.2 Container layout with `indent=2` (verified)

```
{
  "a": [],          <- empty list is inline "[]", no inner newline
  "b": {},          <- empty dict is inline "{}", no inner newline
  "c": [
    1               <- non-empty containers break one element per line
  ],
  "d": {
    "x": 1
  }
}
```

Empty `[]`/`{}` are emitted with no interior whitespace or newline. Non-empty
containers put each element/pair on its own line indented by `2 * depth` spaces.
The closing bracket sits at the parent indent level.

### 1.3 `ensure_ascii` (default True) — Unicode escaping (verified)

Default (True) escapes every non-ASCII char to `\uXXXX` (lowercase hex,
surrogate pairs for astral chars). Example:
`json.dumps({'u':'café ünïcode →'})` →
`{"u": "café ünïcode →"}`.

`ensure_ascii=False` emits raw UTF-8 bytes: `{"z": "café"}`.

RAC sites that pass `ensure_ascii=False` (emit raw UTF-8): all of
`documents_jsonl`, `coverage.py:141`, `doctor.py:429`, `eval.py:535/540`,
`usage.py` (72/167/195), `mcp/telemetry.py`, `mcp/budget.py`, `mcp/audit.py`,
`index_store.py:331`, and the `okf`/human text (those don't go through
`json.dumps` at all). **Every renderer in `output/json.py` EXCEPT
`render_documents_jsonl` uses the default `ensure_ascii=True`** — so `rac
find --json`, `rac export --json`, `rac stats --json`, etc. escape non-ASCII to
`\uXXXX`, but `rac export --documents` (JSONL) emits raw UTF-8. This split is a
top parity landmine.

Standard string escapes (both modes): `"` → `\"`, `\` → `\\`, control chars
`\b \t \n \f \r`, other C0 → `\u00XX`. Forward slash `/` is NOT escaped by
`json.dumps` (only the Portal post-processor escapes `</`, see §6).

### 1.4 Float representation (CRITICAL landmine)

Python's `json` encoder emits floats via `float.__repr__`, which is the
**shortest string that round-trips** to the same IEEE-754 double (the
David-Gay / dtoa "shortest" algorithm). Ints stay ints; floats always carry a
decimal point or exponent. Verified examples:

| Python value | JSON emitted |
| --- | --- |
| `0.1` | `0.1` |
| `1e-05` | `1e-05` (NOT `0.00001`) |
| `1e20` | `1e+20` (explicit `+`, lowercase `e`) |
| `1.0` | `1.0` |
| `100.0` | `100.0` |
| `1234567.0` | `1234567.0` (no exponent) |
| `round(2/3,1)` | `0.7` |
| `round(10/3,1)` | `3.3` |
| `round(1/3,2)` | `0.33` |
| `float('inf')` | `Infinity` (invalid JSON, but Python emits it) |

Exponent rules: lowercase `e`; sign always present (`e+20`, `e-05`); exponent
is **zero-padded to at least 2 digits** (`1e-05`, not `1e-5`). The threshold for
switching to exponent form is Python's repr threshold: `< 1e-4` or `>= 1e16`
roughly. The Rust port MUST use a shortest-round-trip formatter (e.g. `ryu`)
AND then reshape to Python's exact style: Rust `ryu`/`{}` differ (`1e-5` vs
`1e-05`, `1e20` vs `1e+20`, `100.0` vs `1e2` in some formatters). Post-process
to match Python `repr(float)` byte-for-byte.

Where floats actually appear in RAC JSON:
- `stats`: `average_requirements_per_feature` = `round(avg, 1)`;
  `unrecognized.files[].confidence` = `round(conf, 2)` (json.py:134, 184).
- `dir_inspect`: `files[].confidence` is **unrounded** — the raw classifier
  float (json.py:214). This can produce a long shortest-repr like
  `0.8571428571428571`. Confirm the exact float the classifier yields (that is
  another brief's contract; this brief only fixes the *formatting*).
- `find`/other services: any score fields inside `to_dict()`.

### 1.5 `round()` is banker's rounding (round-half-to-even)

`round(x, n)` rounds half to even, operating on the actual binary double (so
`round(2.675, 2) == 2.67` because 2.675 is actually 2.6749999…). Verified:
`round(0.5)=0`, `round(1.5)=2`, `round(2.5)=2`, `round(0.125,2)=0.12`,
`round(2.675,2)=2.67`. The Rust port must replicate half-to-even on the stored
double, not naive half-up. Same applies to the `.1f` / `.0%` format specs in
human output (Python's format mini-language also rounds half-to-even).

### 1.6 Trailing newline

Every `output/json.py` renderer RETURNS a string with **no** trailing newline;
the CLI wraps it in `print(...)` (cli.py: e.g. 475, 488, 520, 861, 969, 1029)
which appends exactly one `\n`. Verified: `rac stats rac/ --json` ends
`...\n  }\n}\n` (the final `}` then a single `\n`). JSONL (`documents_jsonl`)
returns `"\n".join(...)` (no trailing newline internally); `print` adds one, so
the file ends with a single `\n` after the last record and there is NO blank
trailing line.

Sites that write to files instead of print (`preferences.py:89`,
`workspace.py:105`, `ping.py:77`, `consent.py:105`) append `"\n"` explicitly
inside `write_text`.

---

## 2. Stdout encoding / locale independence

There is **no** `sys.stdout.reconfigure(...)`, no `PYTHONIOENCODING` handling,
no `codecs` wrapper anywhere in `src/asdecided/*.py` (grep verified: zero matches).
Consequences:

- Python opens `sys.stdout` as a text stream using the locale preferred
  encoding with the default error handler. For the vast majority of RAC output
  this is irrelevant because `ensure_ascii=True` guarantees pure ASCII bytes.
- The bytes are locale-sensitive **only** for output that contains raw
  non-ASCII: the JSONL documents export (`ensure_ascii=False`), and human/OKF
  text that contains the literal glyphs in §4/§5 (`↳ — → ✗ ✅ ⚠️ ℹ️`).
- Under `LC_ALL=C` / `POSIX`, CPython 3.7+ auto-enables **UTF-8 Mode** (PEP 540),
  so stdout still encodes UTF-8 — non-ASCII glyphs are NOT mangled. UNVERIFIED
  edge: an explicit `PYTHONIOENCODING=ascii` would make the glyph paths raise
  `UnicodeEncodeError` at print time; the Rust port (always UTF-8) would not.
  Treat "always emit UTF-8 bytes" as the contract.
- `BrokenPipeError` handling (cli.py:2480–2488): on a downstream-closed pipe the
  CLI silences the error, points stdout's fd at `/dev/null`, and returns exit
  code `1`. The Rust port should exit non-zero (1) quietly on `EPIPE` too.

---

## 3. Exit-code mapping (convention across commands)

Constants (cli.py:156–158):

```
EXIT_OK               = 0
EXIT_VALIDATION_FAILED = 1
EXIT_USAGE            = 2
```

Conventions:
- **0** — success / valid.
- **1** — the command ran but its subject failed the check: invalid artifact(s),
  failed gate, broken relationships, rename rejected (a *content* failure, NOT a
  usage error). Pattern in handlers: `return EXIT_OK if report.ok else
  EXIT_VALIDATION_FAILED` (cli.py:271, 296, 305, 340, 566, 646, 668, 709…).
  `has_errors(issues)` gates single-file validate (305).
- **2** — usage / argument / IO error. Emitted via `_usage_error()`
  (cli.py:161–173) which prints `rac: <message>` to **stderr** then
  `raise SystemExit(EXIT_USAGE)`. Also argparse's own errors exit 2.
  Namespaced variants keep their own prefix: eval uses `rac eval: <msg>`
  (cli.py:1187), schema unknown-name uses the rendered blob (516).
- `main()` (cli.py:2464) returns the handler's int; `__main__` wraps
  `raise SystemExit(main())`. Telemetry recording in the `finally` never
  changes the exit code (ADR-032).
- `rename` explicitly documents: dry-run refusal returns
  `EXIT_VALIDATION_FAILED` (1), not 2 (cli.py:588, 607–610); the human plan is
  printed to **stderr** (604) while JSON goes to stdout (602).

stderr vs stdout: error/usage lines and some advisory renders go to stderr
(`rac: …`, rename human plan, watchkeeper github annotations at 736, resolve
not-found at 1040). Machine output (`--json`, `--sarif`) always goes to stdout.

---

## 4. Human output formatting (`output/human.py`)

### 4.1 Color / ANSI (verified mechanism)

- `_USE_COLOR = sys.stdout.isatty()` is evaluated **once at module import time**
  (human.py:93). Color is enabled iff stdout is a TTY at process start. Piped /
  redirected / captured output → **no ANSI at all**. There is NO `NO_COLOR` /
  `FORCE_COLOR` / `--color` override — isatty is the sole switch.
- `_c(text, code)` (human.py:96–99): when color on, wraps as
  `"\033[{code}m{text}\033[0m"` (ESC = `0x1b`). Codes used: green `32`, red
  `31`, yellow `33`, bold `1`. `_bold(_red(...))` nests, producing
  `\033[31m\033[1mTEXT\033[0m\033[0m` (outer applied last wraps inner). For
  parity testing pipe stdout (non-tty) so color is off; the Rust port should
  likewise gate on isatty and produce identical ESC sequences when a TTY.

### 4.2 Line endings

All human renderers build a `list[str]` and `"\n".join(lines)`; `print` adds the
final `\n`. Pure `\n`, never `\r\n`, on all platforms.

### 4.3 Column alignment / padding (CRITICAL: code-point width)

Dynamic widths use Python str formatting `f"{value:<{width}}"` (left-justify,
space pad). Widths are computed as `max(len(x) for x in rows)` sometimes `+ N`.
Examples:
- stats "Requirements by Feature": `width = max(len(name)) + 4`, then
  `f"{name:<{width}}{count}"` (human.py:333–335).
- index table: `id_w/type_w/title_w = max(len(...))`, joined with `  ` (two
  spaces) between columns, path last, 2-space leading indent (human.py:941–946).
- decisions-for: `id_w`, `status_w`; continuation line indented by
  `f"{' '*id_w}  {' '*status_w}  "` then `↳ applies to:` (human.py:1020–1026).
- fixed-width type columns: `f"{type_name.title():<14} {count}"` (746, 827,
  1289) — pad to 14.

**Landmine:** Python `len(str)` counts **Unicode code points**, not bytes and
not grapheme clusters. The Rust port MUST pad by `str.chars().count()`, NOT
`str.len()` (bytes) and NOT a grapheme library. A title with `café` (4 code
points, 5 UTF-8 bytes) or an emoji (1 code point) pads by its code-point count —
the visual alignment may look wrong but the BYTES must match Python.

### 4.4 Numeric format specs in human text

- `{x:.1f}` — fixed 1 decimal, half-to-even (average_requirements, 322).
- `{x:.0%}` — value × 100, `%` suffix, 0 decimals, half-to-even
  (confidence/coverage/completeness: 452, 499, 529, 759, 769).
- `{x:g}` — general format, strips trailing zeros (points/ceiling, 524).

### 4.5 Literal glyphs and markers in human output

Non-ASCII code points appearing in human renderers (must be emitted as UTF-8):
- `↳` U+21B3 (decisions-for continuation, 1026)
- `—` U+2014 em dash (empty-field placeholder for title/status, 945, 1025, 1047…)
- `→` U+2192 (threshold hint `→ Unknown`, 529)
- `✗` U+2717 (stdin-corpus relationship finding, 185)
- change icons ASCII: `+ ~ -` (1237)
- fixed strings: `PASS`/`FAIL` verdict prefixes (with 2 trailing spaces before
  filename: `PASS  <file>`, human.py:137–139, 168), `error`/`warning` labels,
  `EMPTY_CORPUS_HINT = "No artifacts yet — create your first with: rac
  quickstart"` (124, contains em dash).
- validate summary line: `f"{n} error(s), {m} warning(s)."` (149).

---

## 5. GitHub / Watchkeeper output (`output/github.py`)

- `render_watchkeeper_github` → GitHub-flavoured Markdown (`\n`-joined, no
  trailing newline from the function; the CLI `print`s it at 733). Emoji glyphs:
  `⚠️` (U+26A0 U+FE0F, warning), `ℹ️` (U+2139 U+FE0F, info), `✅` (U+2705).
  These are multi-code-point (base + variation selector) — emit the exact byte
  sequences. Table markdown `| --- | --- | --- |` with backtick-quoted paths.
- `watchkeeper_annotations` → list of workflow-command lines
  `::error file=PATH::MSG` / `::warning …` / `::notice …` (github.py:98–128),
  printed one per line to **stderr** (cli.py:736). Repo paths built with
  `PurePosixPath` (forward slashes always). Order: newly-invalid errors, then
  new relationship-issue errors (first path of a comma-split), then findings.

---

## 6. Portal HTML (`output/portal.py`)

`render_export_html` injects `render_export_json(export)` (the standard
`indent=2`, `ensure_ascii=True` export payload) into the vendored shell's
`<script type="application/json" id="lore-export"></script>` seam. Before
injection `_escape_for_script` applies exactly two replacements, in order:
`"</"` → `"<\\/"` then `"<!--"` → `"<\\u0021--"` (portal.py:70). Raises
`PortalShellMissing` / `PortalSeamMissing` if the packaged shell is absent or
lacks exactly one seam. Output is the shell with one `.replace(_SEAM, populated)`.

## 6b. OKF bundle (`output/okf.py`)

Not stdout — returns `{relative-path: file-contents}` the CLI writes to disk
(`dest.write_text(content, encoding="utf-8")`, cli.py:851). Each file body is
`"\n".join(lines) + "\n"` (single trailing newline). Frontmatter block:
`---\ntype: {OKF_TYPE}\nid: {id}\n[created: …]\n[updated: …]\n[tags: [a, b]]\n
---\n\n{body}` where body is `split_frontmatter(text).body.strip()`. `tags`
rendered as `[{', '.join(tags)}]`. Ordering deterministic (sorted path; log.md
grouped by ISO date, newest first via `sorted(dated, reverse=True)`). `.strip()`
is Python's — strips ASCII whitespace AND Unicode whitespace from both ends
(UNVERIFIED which exact set matters here; Python `str.strip()` with no arg
strips all Unicode whitespace incl. `\xa0`, ` `, etc.).

---

## 7. SARIF output (`output/sarif.py`) — SARIF 2.1.0

`json.dumps(document, indent=2)` (default `ensure_ascii=True`), printed →
trailing `\n`. Document shape:

```json
{
  "version": "2.1.0",
  "$schema": "https://json.schemastore.org/sarif-2.1.0.json",
  "runs": [
    {
      "tool": { "driver": {
        "name": "rac",
        "informationUri": "https://github.com/itsthelore/asdecided-core",
        "version": <rac.__version__>,
        "rules": [ {"id": <code>}, ... ]
      }},
      "results": [ ... ]
    }
  ]
}
```

Key insertion order is exactly: `version`, `$schema`, `runs` at top level;
`name`, `informationUri`, `version`, `rules` inside driver. Rust must preserve
this order (no sort_keys).

- **`driver.version` is `rac.__version__`** (sarif.py:19,194) — a setuptools-scm
  dynamic version (verified oracle emits `"0.1.dev50+g21c8be403"`). This is
  **NOT byte-stable** across build environments. The parity harness must pin /
  substitute the version or compare with it masked. LANDMINE.
- `rules`: `[{"id": code} for code in sorted({ruleId})]` — deduped rule ids,
  **sorted ascending by the default Python string sort** (code-point order,
  case-sensitive: uppercase before lowercase).
- Each `result` (sarif.py:47–60): keys in order `ruleId`, `level`, `message`
  (`{"text": msg}`), `locations` (`[{"physicalLocation": {"artifactLocation":
  {"uri": encoded}[, "region": {"startLine": line}]}}]`). `region` present only
  when line is not None.
- `uri` is `urllib.parse.quote(uri, safe="/")` — percent-encodes spaces and
  non-ASCII, keeps `/` literal. Rust must replicate RFC-3986 `quote` with
  `safe="/"` exactly (space → `%20`, `é` → `%C3%A9` uppercase hex, etc.).
  UNVERIFIED: Python `quote` default `safe='/'` and the always-safe set is
  `A-Za-z0-9_.-~`; everything else percent-encoded uppercase.
- `level` mapping (`_LEVEL`, sarif.py:44): `error→error`, `warning→warning`,
  `info→note`; unknown → `"warning"` (via `.get(level, "warning")`).
- **Results sort key** (sarif.py:175–182): tuple
  `(uri, region.startLine or 0, ruleId, message.text)`. File-level findings
  (no region) sort as line `0`, i.e. ahead of line-anchored ones for the same
  uri. All comparisons are Python default (strings by code point, ints numeric).

---

## 8. Machine-readable call-site table

Format: `file:line | dumps flags | consumer command / notes`. Flags show only
non-default args; `indent=2` noted where set. All default `ensure_ascii=True`
unless `ea=False` shown; none set `sort_keys` except surface.py.

```
src/asdecided/output/json.py:63   | indent=2                    | validate <file> --json (render_validation_json)
src/asdecided/output/json.py:68   | indent=2                    | validate <dir> --json
src/asdecided/output/json.py:78   | indent=2                    | validate - --corpus --json
src/asdecided/output/json.py:86   | indent=2                    | review --json
src/asdecided/output/json.py:94   | indent=2                    | gate --json
src/asdecided/output/json.py:112  | indent=2                    | diff --json
src/asdecided/output/json.py:194  | indent=2                    | stats --json
src/asdecided/output/json.py:201  | indent=2                    | inspect <file> --json
src/asdecided/output/json.py:216  | indent=2                    | inspect <dir> --json
src/asdecided/output/json.py:223  | indent=2                    | improve --json
src/asdecided/output/json.py:230  | indent=2                    | schema --list --json
src/asdecided/output/json.py:234  | indent=2                    | schema <name> --json
src/asdecided/output/json.py:257  | indent=2                    | relationships --json
src/asdecided/output/json.py:268  | indent=2                    | relationships --validate --json
src/asdecided/output/json.py:276  | indent=2                    | rename (dry-run plan) --json
src/asdecided/output/json.py:281  | indent=2                    | rename (applied) --json
src/asdecided/output/json.py:294  | indent=2                    | ingest <file> --json
src/asdecided/output/json.py:330  | indent=2                    | ingest <dir> --json
src/asdecided/output/json.py:338  | indent=2                    | portfolio --json
src/asdecided/output/json.py:346  | indent=2                    | index --json
src/asdecided/output/json.py:358  | indent=2                    | export --json
src/asdecided/output/json.py:369  | ea=False, indent=None (JSONL)| export --documents  (one compact obj/line, raw UTF-8)
src/asdecided/output/json.py:379  | indent=2                    | export --graph
src/asdecided/output/json.py:388  | indent=2                    | export --agent-rules [--check]
src/asdecided/output/json.py:396  | indent=2                    | templates --json
src/asdecided/output/json.py:401  | indent=2                    | new --json
src/asdecided/output/json.py:406  | indent=2                    | init --json
src/asdecided/output/json.py:411  | indent=2                    | quickstart --json
src/asdecided/output/json.py:419  | indent=2                    | resolve --json
src/asdecided/output/json.py:428  | indent=2                    | decisions-for --json
src/asdecided/output/json.py:439  | indent=2                    | find --json [--explain adds evidence]
src/asdecided/output/json.py:447  | indent=2                    | migrate metadata --json
src/asdecided/output/json.py:455  | indent=2                    | skill install --json
src/asdecided/output/json.py:464  | indent=2                    | skill list --json
src/asdecided/output/json.py:469  | indent=2                    | hook install --json
src/asdecided/output/json.py:478  | indent=2                    | hook list --json
src/asdecided/output/json.py:490  | indent=2                    | mcp-stats --json
src/asdecided/output/json.py:498  | indent=2                    | watchkeeper --json
src/asdecided/output/sarif.py:202 | indent=2                    | validate/review/relationships/gate --sarif
src/asdecided/services/coverage.py:141 | ea=False, indent=2     | coverage report --json (service)
src/asdecided/services/agent_rules.py:218 | (multi-line dumps)  | agent-rules internal
src/asdecided/services/index_store.py:331 | ea=False, indent=None | derived index store portfolio blob (internal)
src/asdecided/services/doctor.py:429   | ea=False, indent=2     | doctor --json
src/asdecided/services/eval.py:535     | ea=False, indent=2     | eval scorecard --json
src/asdecided/services/eval.py:540     | ea=False, indent=2     | eval metrics --json (baseline file too, cli 1167)
src/asdecided/services/derived_cache.py:543 | indent=None        | cache manifest file (internal, not stdout)
src/asdecided/usage.py:72    | ea=False, indent=None            | usage telemetry log line (file)
src/asdecided/usage.py:167   | ea=False, indent=2               | usage --json (stdout)
src/asdecided/usage.py:195   | ea=False, indent=2               | usage --share payload
src/asdecided/mcp/telemetry.py:92  | ea=False, indent=None      | mcp telemetry log line (file)
src/asdecided/mcp/telemetry.py:292 | ea=False, indent=2         | mcp telemetry report
src/asdecided/mcp/budget.py:66     | ea=False, indent=None      | mcp budget payload (compact)
src/asdecided/mcp/surface.py:116   | sort_keys=True (token count)| NOT emitted; only len() for token estimate
src/asdecided/mcp/audit.py:224     | ea=False, indent=None      | audit log line (file)
src/asdecided/mcp/audit.py:303     | ea=False (multi-line)      | audit query output
src/asdecided/mcp/ping.py:77       | indent=2                   | ping state file (+ "\n")
src/asdecided/mcp/ping.py:164      | (default compact)          | ping HTTP POST body (network)
src/asdecided/explorer/preferences.py:89 | indent=2             | explorer prefs file (+ "\n")
src/asdecided/explorer/workspace.py:105  | indent=2             | explorer workspace file (+ "\n")
src/asdecided/consent.py:105       | indent=2                   | consent file (+ "\n")
```

Note: `surface.py:116` is the ONLY `sort_keys=True` site and it is used purely
for an `approx_tokens(...)` length estimate — its bytes are never emitted, but
if the Rust port re-implements token estimation it must sort keys there.

---

## 9. Top parity landmines (summary)

1. **Float repr** — must be shortest-round-trip AND reshaped to Python style
   (`1e-05`, `1e+20`, padded 2-digit signed exponent, always-`.0` for whole
   floats). `dir_inspect` confidence is unrounded → arbitrarily long reprs.
2. **`ensure_ascii` split** — all `output/json.py` renderers escape non-ASCII
   (`\uXXXX`) EXCEPT `render_documents_jsonl` (raw UTF-8). Get this backwards
   and every non-ASCII corpus mismatches.
3. **`indent=2` separators** — bare comma `,` before newline (no trailing
   space), `": "` for keys; empty `[]`/`{}` inline. sort_keys is OFF — preserve
   dict insertion order exactly.
4. **`round()` / format specs are half-to-even** on the stored double, not
   half-up (`round(2.5)=2`, `round(2.675,2)=2.67`).
5. **Human padding by code-point count** (`len` = code points), plus color gated
   solely on `stdout.isatty()` at import; SARIF `driver.version` is a non-stable
   scm version that the harness must mask.
