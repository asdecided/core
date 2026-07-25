# 03 — `rac.core.markdown`: Markdown → Product extraction (parity landmine #2)

Source of truth: `/home/user/rac-core/src/rac/core/markdown.py` (frozen oracle).
Parser: **markdown-it-py 4.2.0**, preset `"commonmark"`, one module-level shared
instance (`_PARSER = MarkdownIt("commonmark")`). All behavior below was verified
empirically with `.venv-oracle/bin/python` unless marked UNVERIFIED.

---

## 1. THE CONSUMED SURFACE (read this first — it decides the bake-off)

The module **never renders HTML and never looks at inline children**. It walks the
**flat block-level token stream** and consumes exactly **two token types**:

1. `heading_open` — fields used: `token.type`, `token.tag` (`"h1"`/`"h2"`/other),
   `token.map` (only `map[0]`, and only for extra-title line numbers).
2. `inline` — fields used: `token.content` (the **raw, unrendered** inline source
   text) and `token.map` (only `map[0]`, only for requirement line numbers).

Heading text is taken as `tokens[i+1].content` — the inline token immediately
following `heading_open` (guarded: `if i + 1 < len(tokens) else ""`, unreachable in
practice since headings always emit `heading_open, inline, heading_close`).
Body inline detection: an `inline` token counts as body **iff** the immediately
preceding token is **not** `heading_open`
(`tok.type == "inline" and not (i > 0 and tokens[i-1].type == "heading_open")`).

Every other token type — `paragraph_open/close`, `blockquote_open/close`,
`bullet_list_open/close`, `ordered_list_open/close`, `list_item_open/close`,
`heading_close`, `fence`, `code_block`, `html_block`, `hr` — is **ignored
entirely**. Container nesting is invisible to the walk: a heading inside a
blockquote or list item is a real `heading_open` in the flat stream and **opens a
section exactly like a top-level heading** (verified: `> ## Risks` opens the Risks
section; `- ## Risks` likewise).

In the `commonmark` preset only **paragraphs and headings** produce `inline`
tokens. Therefore the complete consumed surface is:

> **(a)** heading events: level (h1/h2/other), 0-based start line, and the raw
> inline text of the heading (escapes, entities, `**markup**`, backticks, link
> syntax all preserved verbatim);
> **(b)** paragraph events: the raw multi-line inline text (container markers such
> as `> ` and `- ` stripped by the block parser; NOT a verbatim source slice) and
> the paragraph's 0-based start line.

Content in fenced code, indented code, HTML blocks, and link-reference definitions
is captured **nowhere** (so `# lines` inside fences are never headings, and a
requirement indented 4 spaces becomes a `code_block` and silently vanishes —
verified: `## Requirements\n\n    [REQ-001] x\n` yields zero requirements, zero
malformed, `sections == {"requirements": ""}`).

Active block rules in the preset (verified via `md.block.ruler.get_active_rules()`):
`code, fence, blockquote, hr, list, reference, html_block, heading, lheading,
paragraph`. **Both ATX (`heading`) and setext (`lheading`) headings are consumed** —
`heading_open` is emitted for both (setext markup `=` → `h1`, `-` → `h2`).
No tables, no strikethrough, no linkify (commonmark preset).

**Bake-off implication:** any parser (markdown-it.rs, comrak, pulldown-cmark, or a
bespoke block tokenizer) works **iff** it can reproduce (a) CommonMark block
structure decisions (which lines are headings/paragraphs/code/HTML, including
setext, lazy continuation, tab handling, list/blockquote marker stripping, HTML
block types 1–7) and (b) markdown-it's exact `inline.content` string for
paragraphs and headings, plus 0-based block start lines. markdown-it-py 4.x
targets CommonMark spec 0.31.2 (UNVERIFIED exact spec revision — pin against the
package, not the spec).

## 2. Source normalization (inside markdown-it, before block parsing)

- Newlines: `\r\n` and lone `\r` → `\n` (regex `\r\n?|\n`). Token `content` never
  contains `\r`. Verified: CRLF input tokenizes identically to LF with identical
  `map` values.
- `NUL`: `\0` → U+FFFD. Verified: `## Prob\x00lem` → heading text `Prob�lem`
  → section key `'prob�lem'`.
- `token.map` is `[start, end)`: **0-based, end-exclusive** source-line range
  (post frontmatter split). Setext heading map covers text + underline
  (`[0, 2]` for `Title\n=====`), but its inline token's map covers only the text
  line — the walk only ever reads the `heading_open`'s map for h1s, and the
  `inline`'s map for body, so this distinction matters.

### markdown-it `inline.content` shape (verified)

- ATX heading: text between the opening `#…` run and any trailing closing-`#` run,
  with surrounding **spaces/tabs** trimmed by the parser. `## Risks ##`,
  `## Risks ####   ` → `'Risks'`. `#\tFoo` → `'Foo'`. Up to 3 leading spaces
  allowed (`   ## Indented` is a heading); a tab indent makes it a `code_block`.
  `#nospace` and `#5 bolts` are **not** headings (paragraphs). `\## x` is a
  paragraph; `## has \# escaped` → content `'has \\# escaped'` (backslash kept).
  Entities kept raw: `## AT&amp;T &#65;` → `'AT&amp;T &#65;'`. Empty `##` →
  content `''`.
- Paragraph: lines joined with `\n`; **first line's leading** and **last line's
  trailing** space/tab trimmed; interior lines keep their spacing; hard-break
  trailing spaces and trailing `\` retained (`'foo  \n  bar\\\nbaz  \nqux\thard'`).
  Container markers removed (blockquote `> `, list markers `- ` / `1. ` and item
  indentation). This trimming is unobservable downstream except through the
  char-budget accounting, because every consumed line is re-stripped in Python.

## 3. The walk (exact algorithm)

State: `offset` (frontmatter line offset), `title=None`, `extra_title_lines=[]`,
`section=None`, `current_h2=None`, `search_sections=[]`, `current_search=None`,
`problem_lines=[]`, `requirement_lines=[]`, `metric_lines=[]`, `risk_lines=[]`,
`section_bodies={}` (insertion-ordered), `section_chars={}`, `captured_lines=0`,
`truncated_fields=set()`, `body_truncated=False`,
`has={"problem":F,"requirements":F,"success_metrics":F,"risks":F}`.

For each token index `i`:

**`heading_open`** → `open_heading(tok, heading_text)` where
`heading_text = tokens[i+1].content`:

- `tag == "h1"`: if `title is None`: `title = heading_text.strip()` (Python strip —
  see §6; note empty `#` sets `title = ''`, which is **not** None, so a later real
  h1 becomes an *extra* title — verified). Else append
  `tok.map[0] + 1 + offset` (or `0` if `map is None`) to `extra_title_lines`.
  Then `section = None`, `current_h2 = None`, `current_search = None` — **content
  under any h1 (until the next h2) is captured nowhere**.
- `tag == "h2"`: `normalized = heading_text.strip().casefold()`;
  `current_h2 = normalized`; `section_bodies.setdefault(normalized, [])` (so an
  empty h2 section still appears in `sections`, and a duplicate normalized heading
  **merges** into the existing key at its first position);
  `current_search = SearchSection(heading=heading_text.strip())` — a **new**
  SearchSection per h2 occurrence, heading in original case, **appended even for
  duplicates and even for empty-string headings**;
  `section = {"problem":"problem","requirements":"requirements",
  "success metrics":"success_metrics","risks":"risks"}.get(normalized)`;
  if recognized, `has[key] = True` (never gated by truncation).
- any other tag (h3–h6): `section = "other"` only. **`current_h2` and
  `current_search` are NOT reset** — body under an h3 still flows into the
  enclosing h2's generic body and search lines, but not into typed fields.
  The h3+ heading's own text is dropped entirely. An h3 before any h2 → content
  dropped (`current_h2 is None`). Verified.

**body `inline`** → `capture_inline(tok)`:

1. If `body_truncated`: return (capture fully stopped; headings still processed —
   later sections still register `has` flags, empty `sections` entries, and empty
   SearchSections. Verified.)
2. If `current_h2 is not None`: **generic capture** into
   `section_bodies[current_h2]` (§4).
3. If (now) `body_truncated`, or `section is None`, or `section == "other"`:
   return. Else **field capture** (§5).

Note the order: generic capture can trip the line ceiling mid-token, which then
suppresses field capture for that same token.

## 4. Generic body capture (`_capture_generic_body`) — the ONLY place budgets apply

For each `raw` in `tok.content.split("\n")`:
- `stripped = raw.strip()` (Python strip, §6); skip if empty.
- If `captured_lines >= 50000` (`MAX_CAPTURED_LINES`): set `body_truncated = True`
  and **break** (stops all further capture of any kind).
- If `section_chars.get(heading, 0) + len(stripped) > 262144` (`MAX_FIELD_CHARS`,
  `len` = **Unicode code points**, not bytes): add `heading` to `truncated_fields`
  and `continue` — **the cap is per line, not a hard stop**: a later, shorter line
  under the remaining budget IS still captured (verified: 200k-line kept, 200k-line
  dropped, 10-char line kept).
- Else append `stripped` to `section_bodies[heading]`;
  `section_chars[heading] += len(stripped) + 1`; `captured_lines += 1`;
  if `current_search is not None`: append `stripped` to `current_search.lines`.

Budget accounting keys off the **normalized** heading, so duplicate `## Problem` /
`## PROBLEM` sections share one char budget.

## 5. Typed field capture (`_capture_field`) — NO char cap here (landmine)

`start_line = tok.map[0] + offset` (or `0` if `map is None` — UNVERIFIED trigger,
believed unreachable for paragraph inlines). Then
`_content_lines(tok.content, start_line)` yields
`(raw_line.strip(), start_line + line_index_within_content + 1)` for each
non-blank stripped line (blank lines advance the counter, keeping numbers
accurate). Content lines map 1:1 to source lines for paragraphs (soft breaks,
lazy continuations, blockquote/list stripping all preserve line count — verified
for blockquote and loose/ordered lists).

Routing by `section`:
- `"problem"` → append texts to `problem_lines`
- `"requirements"` → append `(text, line)` pairs to `requirement_lines`
- `"success_metrics"` → texts to `metric_lines`
- `"risks"` → texts to `risk_lines`

**The 262144-char field cap is NOT applied here.** A 300,000-char line dropped
from `sections["problem"]` still lands in full in `product.problem`, and a
300,000-char `[REQ-001] …` line still becomes a full `Requirement` (verified:
`sections['problem'] == 'short'` while `product.problem` is 300,006 chars). Only
the 50,000-line ceiling (via the early `body_truncated` return) gates field
capture. `product.problem`/`requirements`/`success_metrics`/`risks` can therefore
**diverge from `product.sections`** under the char cap.

## 6. Python string semantics you must replicate

- **`str.strip()`** (used on heading text, titles, section-body lines, field
  lines, bracket ID, description): strips chars where Python `str.isspace()` is
  True. This includes ASCII whitespace, U+00A0 NBSP, U+0085, Unicode spaces, **and
  U+001C–U+001F (FS/GS/RS/US)**. Rust `char::is_whitespace` (Unicode White_Space)
  does **NOT** include U+001C–U+001F — verified divergence risk:
  `## \x1cProblem\x1f` normalizes to `problem` and IS recognized. Interior
  whitespace is never touched: `## Problem x` (NBSP not trimmed by
  markdown-it, interior after strip) → key `'problem x'`, NOT recognized;
  but a **trailing** NBSP is stripped by Python (`## Problem ` → recognized)
  even though markdown-it left it in the token.
- **`str.casefold()`** (heading normalization): full Unicode case folding, not
  lowercase. Verified: `## PROBLEMß` → key `'problemss'`; `## İstanbul` → key
  `'i̇stanbul'` (U+0069 + U+0307). Rust must use full case folding (e.g. the
  `caseless`/ICU full fold), never `to_lowercase()`.
- **`re` patterns** (applied to the already-stripped line):
  - `_BRACKET_RE = ^\[(?P<id>[^\]]*)\]\s*(?P<text>.*)$` — `re.match` (anchored at
    start; `$` also matches before a trailing `\n`, irrelevant post-split). `\s`
    is **Unicode** whitespace. Empty ID `[]` matches (bad_id `''`).
  - `_CANONICAL_ID_RE = ^REQ-\d+$` — `\d` is **Unicode Nd**: `[REQ-١٢٣]`
    (Arabic-Indic) and `[REQ-１２]` (fullwidth) are **VALID** requirement IDs,
    preserved verbatim (verified). Rust `regex` `\d` is Unicode `\p{Nd}` by
    default — do not "fix" to `[0-9]`.
- **`repr()` of the heading** appears inside the `field-truncated` message
  (`f"section {heading!r} …"`): single-quoted by default; switches to double
  quotes when the string contains `'` but no `"` (verified:
  `section "it's" exceeds …`); non-printable chars escaped (`\x07`), printable
  non-ASCII kept literal. Full Python `repr` string rules required.
- Integer formatting: `262144`, `50000`, `1048576` — plain decimal, no separators.

## 7. Requirement line classification (`_classify_requirement_line`)

Input: one stripped line + its 1-based file line number.
1. No `_BRACKET_RE` match (line doesn't start with `[`…`]`) →
   `MalformedRequirement(raw=line, line=n, bad_id=None, empty_text=False)`.
   Note: bullet markers were already stripped by the block parser, so
   `- [REQ-001] x` classifies as a valid requirement, not malformed.
2. `req_id = group("id").strip()`, `desc = group("text").strip()`
   (`[ REQ-001 ]   spaced desc  ` → id `REQ-001`, text `spaced desc` — verified).
3. `req_id` fails `^REQ-\d+$` → `MalformedRequirement(bad_id=req_id)` (e.g.
   `req-1`, `''`).
4. Valid ID but empty desc → `MalformedRequirement(bad_id=req_id, empty_text=True)`.
5. Else `Requirement(id, text, line)`.

Ordering: `requirements` and `malformed_requirements` are two lists partitioned
from `requirement_lines` in document order.

## 8. Product assembly (end of `parse`)

- `problem`: `"\n".join(problem_lines).strip()` **iff** `has["problem"]` else
  `None`. Three-state: `None` = no `## Problem` heading; `""` = heading present,
  no body (also when a setext trap consumed the body, see §10); else joined text.
  Note the extra outer `.strip()` on the join.
- `success_metrics` = `metric_lines`, `risks` = `risk_lines` (lists of stripped
  lines; no has-gating — absent section ⇒ empty list; distinguish via
  `has_metrics_section`/`has_risks_section`).
- `sections` = `{normalized_heading: "\n".join(lines)}` in **first-occurrence
  insertion order** (Python dict). Duplicate normalized headings merge their
  (capped) lines under one key. Empty section ⇒ `""` value.
- `search_sections`: one entry **per h2 occurrence** in document order:
  `heading` = original-case stripped heading text, `lines` = the same
  budget-capped stripped lines that went into `section_bodies` for that
  occurrence.
- `has_*_section` flags from `has`.
- `parse_issues` = `_budget_issues`: first one
  `Issue("warning", "field-truncated", f"section {heading!r} exceeds the "
  f"262144-char field cap and was truncated", line=None)` per truncated heading,
  in `sorted()` order of the normalized headings (code-point sort; verified
  `alpha` before `zeta`); then, if `body_truncated`,
  `Issue("warning", "body-truncated", "document body exceeds the 50000-line "
  "capture cap and was truncated", line=None)`.

## 9. `parse(text, source_path="")` envelope

1. `cap = max_file_bytes()`: `DECIDED_MAX_FILE_BYTES` env override (int; `<= 0` or
   unparseable → default `1048576`).
2. Byte cap check `exceeds_byte_cap(text, cap)` — measured in **UTF-8 bytes** of
   the decoded text (fast path: `len(text) > cap` → over;
   `len(text) <= cap // 4` → under; else encode and compare). Over →
   degraded `Product(title=None, source_path=…, parse_issues=[Issue("error",
   "artifact-oversize", f"artifact exceeds the {cap}-byte parse cap (set "
   "DECIDED_MAX_FILE_BYTES to raise it)", 1)])` — note **"parse cap"** wording,
   distinct from `parse_file`'s "file cap"; everything else default/empty.
3. `split_frontmatter(text)` (splits on `\n`; opener line 0 must strip to `---`;
   closer strips to `---` or `...`): `body` excludes the block,
   `line_offset = closer_index + 1` (= number of removed lines); no frontmatter →
   offset 0; unterminated opener → whole text as body, offset 0, plus
   `Issue("error", "malformed-frontmatter", "frontmatter block opened with ---
   on line 1 but never closed", 1)` in `metadata_issues` (NOT `parse_issues`).
   With frontmatter, `parse_frontmatter(raw)` fills `metadata`/`metadata_issues`
   (contract in the frontmatter section, not here).
4. Tokenize the **body**; every reported line = body line + `offset` (verified:
   3-line frontmatter ⇒ `[REQ-001]` on file line 6).

## 10. Edge-case catalog (all verified unless noted)

| Input | Result |
|---|---|
| `## Risks ##` / `## Risks ####   ` | heading text `Risks` (closer run + ws removed by parser) |
| `\## not heading` | paragraph (body line `\## not heading` captured raw, backslash kept) |
| `## has \# escaped` | key `'has \\# escaped'` (backslash in key and search heading) |
| `## **Bold** \`code\` [link](x) *em*` | heading text kept **raw with markup**: key `'**bold** `code` [link](x) *em*'` |
| `Title\n=====` | setext h1 → title `Title` |
| `Some paragraph\n---` | **setext trap**: h2 `some paragraph`, no paragraph captured |
| `## Problem\nthe text\n---\n` | Problem body becomes an h2! `problem` → `''`, phantom section `'the text'` appears, `has_problem_section=True` |
| `- Sub\n  ---` | setext h2 **inside a list item**: section `'sub'` |
| `> ## Risks\n> - risk one` | Risks section opened from inside blockquote; `risk one` captured (marker-stripped) |
| `#\tFoo` | h1 `Foo`; `\t# Foo` | indented code block, invisible |
| `   ## Indented` | valid h2 (≤3 spaces) |
| `#nospace`, `#5 bolts` | paragraphs, not headings |
| `##` (empty) | h2 with text `''` → `sections['']`, SearchSection heading `''` |
| `#` then `# Real` | `title == ''` (not None!), `Real` recorded as extra title line |
| fence/indented code/html_block/hr/ref-defs | never captured, never headings |
| `<b>inline html</b>` in a paragraph | captured raw as text |
| CRLF file | identical output to LF (parser normalizes; frontmatter `.strip()` handles `\r`) |
| `\0` in source | U+FFFD in captured text and section keys |
| content under h1 / before any heading / under h3-with-no-h2 | captured nowhere |
| h3–h6 under an h2 | body flows to the h2's `sections`/search entry; **not** to typed fields; heading text itself dropped |
| duplicate `## Problem`/`## PROBLEM` | one `sections['problem']` (merged, shared char budget); separate SearchSections `('Problem',…),('PROBLEM',…)` |

## 11. `parse_file(path)`

1. `cap = max_file_bytes()`; `os.path.getsize(path)`; `size > cap` → degraded
   Product with `Issue("error", "artifact-oversize", f"artifact exceeds the
   {cap}-byte file cap (set DECIDED_MAX_FILE_BYTES to raise it)", 1)` — **"file
   cap"** wording here.
2. `open(path, "rb").read(cap + 1)`; any `OSError` (stat or read) → degraded
   Product with `Issue("error", "unreadable-artifact", f"cannot read artifact:
   {exc}", 1)`. The message embeds Python's OSError str, e.g.
   `cannot read artifact: [Errno 2] No such file or directory:
   '/nonexistent/file.md'` — format `[Errno N] <strerror>: '<path>'` with the
   path **repr-quoted**. Rust must reproduce errno text byte-for-byte
   (platform-dependent; pin to Linux strings).
3. `len(data) > cap` (file grew / symlink) → same file-cap oversize issue.
4. `data.decode("utf-8")`; on `UnicodeDecodeError`, re-decode with
   `errors="replace"` (U+FFFD per maximal invalid subpart — matches Rust
   `from_utf8_lossy` on all probed cases: `b"\xe2\x82"`→1×FFFD, `b"\xc3("`→FFFD+`(`,
   `b"\x80abc"`→FFFD+`abc`; exhaustive equivalence UNVERIFIED — fuzz it), parse,
   then **append** `Issue("warning", "non-utf8-content", "artifact is not valid
   UTF-8; decoded lossily", 1)` after any budget issues in `parse_issues`.
5. Note: `parse` re-checks the byte cap on the decoded text ("parse cap" message);
   with `read(cap+1)` short-reads a lossy decode can't exceed it in practice.

## 12. Constants (from `limits.py`)

`DEFAULT_MAX_FILE_BYTES = 1048576` (env `DECIDED_MAX_FILE_BYTES`),
`MAX_FIELD_CHARS = 262144` (code points, per normalized-h2 section),
`MAX_CAPTURED_LINES = 50000` (total non-blank captured lines per document).
Frontmatter caps (`MAX_FRONTMATTER_BYTES = 65536`, `MAX_FRONTMATTER_DEPTH = 32`)
apply inside `parse_frontmatter`, specified elsewhere.

## 13. Top parity landmines (ranked)

1. **Char cap divergence**: `sections`/`search_sections` are budget-capped;
   `problem`/`requirements`/`success_metrics`/`risks` are NOT (only the line
   ceiling stops them). Per-line skip, not hard stop.
2. **Setext headings consumed**: `text\n---` silently converts body text into an
   h2 section anywhere, including inside sections and list items.
3. **`casefold` + Python `strip`**: full case folding (`ß`→`ss`, `İ`→`i`+U+0307)
   and Python's whitespace set (includes U+001C–U+001F, which Rust `trim` lacks).
4. **Headings inside blockquotes/lists open sections**; bullet/quote markers are
   stripped from captured body lines (requirement lines arrive marker-less).
5. **Raw inline content, not rendered text**: markup, escapes, entities, inline
   HTML all preserved verbatim in heading keys, search headings, and body lines.
6. **`\d` is Unicode**: `REQ-١٢٣` is a valid ID.
7. **Exact issue strings** embed Python `repr` (quote-switching) and Python
   OSError formatting.
8. **Empty-string title from `#`** blocks later h1s (title `''` is not None).
