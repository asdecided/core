# 06 — Resolve & Search (`rac resolve`, `rac find`) — `src/asdecided/services/resolve.py`

Scope: exact-ID resolution, tiered token-boundary search matching (ADR-037/038/109),
BM25F + RRF deterministic ranking (ADR-078), and the `resolve`/`find` CLI output
contracts (human + JSON, exit codes). All numeric examples below were verified
against the live `rac/` corpus with `.venv-oracle/bin/rac` on 2026-07-11
(corpus n=418 artifacts) — re-derive them after any corpus change; the *formulas*
are the contract, the numbers are regression anchors.

Upstream inputs this section treats as given (see the index/identity/corpus
sections): each searchable entry (`IndexEntry`) provides
`id: str`, `type: str`, `title: Option<str>`, `path: str`,
`aliases: Vec<str>` (canonical ID first, then legacy aliases — declared `## ID`,
filename `<letters>-<digits>` prefix, filename stem; case-insensitively deduped),
`search_sections: Vec<SearchSection { heading: str, lines: Vec<str> }>`
(headings as written; non-blank body lines, each whitespace-stripped, in document
order), `inbound_count: i64` (resolved inbound relationship edges), and
`tags: Vec<str>` (frontmatter order). Entries appear in corpus-walk order.
`unknown`-type documents ARE in the index and ARE searchable and counted in
corpus statistics.

---

## 1. Tokenization (`tokenize`) — ADR-037

```python
_NON_ALNUM_RE = re.compile(r"[^0-9A-Za-z]+")
_CAMEL_RE     = re.compile(r"(?<=[a-z])(?=[A-Z])")

def tokenize(text):
    tokens = []
    for piece in _NON_ALNUM_RE.split(text):      # split on runs of non-ASCII-alnum
        if not piece: continue
        for sub in _CAMEL_RE.split(piece):       # split at lowercase→uppercase seam
            if sub: tokens.append(sub.casefold())
    return tokens
```

Rules, in order:

1. Split on maximal runs of characters **not** in `[0-9A-Za-z]` (ASCII-only!).
   Every non-ASCII character — including Unicode letters, combining marks,
   em-dashes — is a separator. Drop empty pieces.
2. Split each piece at every position where an ASCII lowercase letter
   (`[a-z]`) is immediately followed by an ASCII uppercase letter (`[A-Z]`).
   Digits do not trigger camel splits; uppercase→uppercase does not split.
3. `str.casefold()` each sub-piece. Because pieces are pure ASCII alnum after
   step 1, casefold here is exactly ASCII `A-Z → a-z` — full Unicode casefold
   is unreachable in `tokenize` (but IS reachable in `resolve` and the tag
   facet, see §3 and §5.2).

Verified examples:

| input | tokens |
|---|---|
| `soft-delete` | `[soft, delete]` |
| `camelCase` | `[camel, case]` |
| `HTTPServer` | `[httpserver]` |
| `MiXeD-Case_fooBAR` | `[mi, xe, d, case, foo, bar]` |
| `v0.22.0` | `[v0, 22, 0]` |
| `ADR-037` | `[adr, 037]` |
| `foo_barBaz2Qux` | `[foo, bar, baz2qux]` |
| `café` (precomposed U+00E9) | `[caf]` |
| `éclair` (decomposed e + U+0301) | `[e, clair]` |
| `İstanbul` | `[stanbul]` |
| `Straße` | `[stra, e]` |
| `...`, `""` | `[]` |

Query terms are `tokenize(query)`; **duplicates are NOT deduped** for scoring
(landmine, §7.1).

## 2. Term↔token matching predicate and term frequency

- Match predicate (`_term_hits_tokens`): term `t` hits token list `T` iff
  `∃ token ∈ T: token == t or token.startswith(t)` — i.e. **the query term is a
  prefix of (or equal to) the token**, one-directional. `sear` matches
  `searching`; `searching` does not match `sear`. No substring matching.
- Term frequency (`_tf(term, tokens)`): the **count** of tokens for which
  `token == term or token.startswith(term)` — prefix hits count into tf.

## 3. Exact resolution — `rac resolve <ID> [directory]`

Service (`resolve_artifact` → `resolve_in_index`):

1. `wanted = artifact_id.strip().casefold()` — Python `str.strip()` strips
   Unicode whitespace (not just ASCII); `str.casefold()` is **full Unicode
   casefold** (e.g. `ß → ss`, `İ → i̇` = `i` + U+0307). Rust must use Unicode
   case folding + Unicode whitespace trim, not ASCII lowercase/trim.
2. An entry matches iff **any** alias satisfies `alias.casefold() == wanted`
   (each alias is casefolded too). Exact equality only — no prefix, no tokens.
3. Outcomes:
   - 0 matches → `not-found`
   - ≥2 matching **entries** (distinct files) → `duplicate`, with
     `duplicate_paths = sorted(paths)` (Python `str` sort = code-point order)
   - exactly 1 → `resolved`

Resolution reads only aliases/path/id/type/title; it never builds sections or
the graph (perf-only; no behavioral effect).

### 3.1 CLI contract

- Usage guard first: if `directory` is not a directory →
  stderr `rac: not a directory: {directory}\n`, exit **2**.
- `--json` (resolved): keys in this exact order, `json.dumps(..., indent=2)`
  plus one trailing `\n` from `print`:

  ```json
  {
    "schema_version": "1",
    "id": "RAC-KTXTAF6ZKDK8",
    "type": "decision",
    "title": "ADR-037: Token-Boundary Search Matching",
    "path": "rac/decisions/adr-037-token-boundary-search-matching.md"
  }
  ```

  `title` is `null` when absent. `section`/`snippet`/`evidence`/`recency`/`tags`
  are **never** emitted by resolve (always None/empty on the resolution path).
- `--json` (not found): `{"schema_version": "1", "error": "not-found", "id": "<query as given, unstripped>"}` — exit 1.
- `--json` (duplicate): same plus `"paths": [ ...sorted... ]` after `id`; exit 1.
- Human (resolved), to stdout, exit 0 (`{id}` wrapped in `\x1b[1m…\x1b[0m` only
  when stdout is a tty; plain when piped):

  ```
  RAC-KTXTAF6ZKDK8

  Type: decision
  Title: ADR-037: Token-Boundary Search Matching
  Path: rac/decisions/adr-037-token-boundary-search-matching.md
  ```

  Missing title renders `—` (U+2014).
- Human (not found), to **stderr**: `rac: artifact not found: {id}\n`, exit 1.
- Human (duplicate), to **stderr**, exit 1:

  ```
  rac: duplicate artifact ID: ADR-900

  Found in:
  - x/adr-900-dup.md
  - y/adr-900-dup.md
  ```

- Exit codes: 0 resolved, 1 not-found/duplicate, 2 usage. `--json` still exits 1
  on not-found/duplicate.
- Verified: `rac resolve " adr-037 " rac/` resolves (strip + casefold).

## 4. Search matching — the tier ladder

Query terms `terms = tokenize(query)`. If `terms` is empty (empty/all-punctuation
query) there are **no matches** (valid empty result, exit 0).

Per candidate entry (after the `--type` and `--tag` pre-filters, §5), tokenize
each field once:

- `id`: concatenation of `tokenize(alias)` for every alias, in alias order.
- `title`: `tokenize(title or "")`.
- `tags`: `tokenize(tag)` for each tag, concatenated in tag order
  (`data-model` → `[data, model]`).
- `path`: `tokenize(path)` (so `.md`, directory names, and the stem all contribute;
  note `md` is a token of every path).
- `heading`: concatenation of `tokenize(sec.heading)` over sections in document order.
- `body`: concatenation of `tokenize(line)` over every section's lines in document
  order. Tokens never cross line boundaries.

Tier ladder, evaluated in this fixed order (rank = tier number):

| rank | field | snippet? |
|---|---|---|
| 0 | id | no |
| 1 | title | no |
| 2 | tags | no |
| 3 | path | no |
| 4 | heading | yes: `(section, snippet) = (heading, heading)` of the **first section in document order** with ≥1 term hit |
| 5 | body | yes: `(section, snippet) = (sec.heading, line)` of the **first matching line in document order** |

Algorithm (`_match_entry`): run every tier; union the per-tier matched-term sets
into `matched_terms`; `best_rank` = the first (lowest) tier with ≥1 hit; remember
each snippet-bearing tier's first snippet.

- **AND semantics**: the entry matches iff `set(terms) ⊆ matched_terms`
  (every distinct query term matched *somewhere*, possibly in different fields).
- Only the **winning tier's** snippet is surfaced. A metadata-tier win (rank ≤ 3)
  has `section = snippet = None` even when heading/body also matched.
- Evidence terms: distinct terms **in query-token order** (`dict.fromkeys(terms)`
  filtered to matched — under AND this is every distinct term).
- Evidence tier/field: the winning rank and its name
  (`id/title/tags/path/heading/body`).

Note the heading snippet duplication (verified): a heading win emits
`section == snippet == the heading text`, so human output shows e.g.
`↳ Unified Search and Commands: Unified Search and Commands`.

## 5. Filters

### 5.1 `--type TYPE`

Skip entries with `entry.type != TYPE` **before matching**. String equality,
case-sensitive, no validation of the type name (an unknown type yields an empty
result, exit 0). Mutually exclusive with `--decisions` (argparse error, exit 2).

### 5.2 `--tag TAG` (repeatable) — ADR-109 facet

`tag_filter = frozenset(casefold(t) for t in tags)`; entry passes iff
`tag_filter ⊆ {casefold(tag) for tag in entry.tags}` — **exact whole-tag**
comparison (full Unicode casefold, NOT tokenized: `--tag data-model` never
matches via the `model` token). AND across repeated `--tag`. Applied before
matching/scoring, alongside `--type`.

### 5.3 `--decisions` — ADR-067 live-decision query

Runs `search_index(entries, topic, artifact_type="decision")`, then **post-filters**
matches to live decisions (Accepted, non-retired — predicate owned by
`agent_rules.is_live_decision`, see that section), preserving order.
Consequences (quirks to reproduce):
- Ranks/scores in evidence are computed over **all matched decisions including
  non-live ones**, so surfaced `lexical_rank`/`graph_rank` may have gaps.
- JSON `"type"` field is `"decision"` (not null).
- `--tag` is **silently ignored** on the `--decisions` path (both cached and
  fresh code paths omit it).

## 6. Corpus statistics (`_corpus_stats`)

Computed over the **whole entry set** — including entries excluded by
`--type`/`--tag` and entries that did not match. Verified: BM25 for a given doc
is byte-identical with and without `--type` (only ranks change, because ranks
are computed over the matched set only).

- `n` = number of entries (dict of per-path field tokens; paths are unique).
- `df[term]` = for **each occurrence of `term` in the terms list** (duplicates
  iterate!), count of docs where any of the 6 fields has `_tf(term, field) > 0`.
  Because `df` is a dict keyed by term, a term appearing twice in the query
  increments the same key twice per matching doc → **df doubles** (§7.1).
- `avglen[field]` = `length_sums[field] / n` — Python int/int true division
  (in Rust: `i64 as f64 / i64 as f64`, IEEE-exact). `0.0` when `n == 0`.
- Field iteration order everywhere is `_FIELD_BOOSTS` insertion order:
  **`id, title, path, heading, body, tags`** — note `tags` is LAST, not at its
  tier position (deliberate: preserves pre-ADR-109 float summation order).

Verified anchors (live corpus, query `search`): `n=418`,
`df["search"]=96`, `avglen = {id: 7.035885167464115, title: 6.399521531100478,
path: 8.748803827751196, heading: 16.00956937799043, body: 743.9808612440191,
tags: 1.007177033492823}`.

## 7. BM25F score — the EXACT f64 operation sequence

Constants: `_BM25_K1 = 1.2`, `_BM25_B = 0.75`, `_RRF_K = 60` (int),
`_GRAPH_WEIGHT = 0.5`, boosts `{id: 4.0, title: 3.0, path: 2.0, heading: 1.5,
body: 1.0, tags: 2.5}` (iteration order as listed — tags last).

Per matched entry (only matched entries are scored):

```text
score = 0.0
for term in terms:                      # QUERY-TOKEN ORDER, DUPLICATES INCLUDED
    d = df.get(term, 0)
    if d == 0: continue
    idf = math.log(1 + (n - d + 0.5) / (d + 0.5))
        # natural log; argument built as: t1 = (n - d)      (int)
        #                                 num = t1 + 0.5    (f64)
        #                                 den = d + 0.5     (f64)
        #                                 arg = 1 + num/den (f64 add of literal int 1)
        # NOT log1p — the 1+x is a plain f64 addition before ln.
    weighted_tf = 0.0
    for name, boost in FIELD_BOOSTS:    # id, title, path, heading, body, tags
        tf = _tf(term, fields[name])    # int, prefix-counting
        if tf == 0: continue            # zero-tf fields are SKIPPED (no +0.0 term)
        length = len(fields[name])      # int
        mean = avglen[name]             # f64
        denom = 1.0 - B + B * (length / mean)  if mean > 0 else 1.0
              # ((1.0 - 0.75) + (0.75 * (length_f64 / mean)))
              # left-to-right: 0.25 first, then B*(len/mean), then add.
              # (len/mean) divides FIRST, then multiplies by 0.75.
        weighted_tf += boost * (tf / denom)    # tf/denom first, then * boost,
                                               # then accumulate IN FIELD ORDER
    if weighted_tf > 0:
        score += idf * (weighted_tf / (K1 + weighted_tf))
              # K1 + wtf first, wtf/(that), then * idf, accumulate IN TERM ORDER
```

`math.log` = C `log` (correctly-rounded-ish libm natural log). Rust `f64::ln`
uses the same underlying libm on glibc targets; this is the one place where a
non-identical libm could diverge — pin it (musl/other libm may differ in the
last ulp). Everything else is plain IEEE-754 ops whose ORDER above is normative.

Associativity bite-points (do not reorder):

- `weighted_tf` accumulation order = `id, title, path, heading, body, tags`,
  skipping zero-tf fields entirely (a skipped field is NOT `+ 0.0`;
  an untagged artifact skips the tags term).
- `score` accumulation order = query-token order, duplicates included.
- `1.0 - B + B*(x)` is `(1.0 - B) + (B * x)`, not `1.0 - (B - B*x)` etc.
- `1 + num/den` before `ln`, never `ln1p(num/den)`.
- `avglen` from a single int sum divided once — do not stream-average.

Verified (query `search`, doc `rac/decisions/adr-038-body-text-search-tier.md`):
per-field `tf/len` = id 1/10, title 1/6, path 1/9, heading 0/11, body 9/540,
tags 0/0; `idf = 1.4683279115771974`; final `bm25 = 1.383102686082548`
(hand-rolled sequence reproduces it bit-for-bit).

### 7.1 LANDMINE — duplicate query terms

`terms` keeps duplicates. Query `"search search"` on the live corpus:
`df["search"] = 192` (double-counted → smaller idf), and the outer loop adds the
(smaller) per-term contribution **twice**: bm25 for the doc above becomes
`1.4652617576887943` (evidence `1.465262`), NOT `2 × 1.383103`. Matching and
evidence terms are unaffected (`terms` evidence = `["search"]`, deduped).

## 8. Graph signal and competition ranks

- `inbound[path] = float(entry.inbound_count)` for matched entries.
- `bm25[path]` as above, for matched entries.
- `_competition_ranks(scores)`: sort items by `(-score, path)` (score negated
  f64, path code-point order). Walk in order with 1-based positions; a new rank
  is assigned only when `score != previous` (**exact f64 equality**); ties share
  the first position's rank ("1224" competition ranking). Equal bm25 floats —
  which occur only when the full op sequence yields bit-equal results — and
  equal inbound counts share ranks; this is where any bm25 bit-divergence
  becomes an ordering divergence.

## 9. RRF fusion, rounding, and the sort

```python
graph_allowed[path] = bm25[path] >= max(bm25.values()) * 0.85
fused[path] = 1.0 / (60 + lexical_rank[path])
if graph_allowed[path]:
    fused[path] += 0.5 / (60 + graph_rank[path])
```

`60 + rank` is integer addition, then f64 division; lexical term first, then
`+` the graph term (one f64 add) only for candidates at or above the inclusive
85% lexical floor. A candidate below the floor retains the lexical term and its
graph contribution is clamped to zero.

Final ordering: `matched.sort(key=lambda em: (-round(fused[path], 12), path))`.

- `round(x, 12)` is Python float round: **correct decimal rounding of the exact
  binary value to 12 decimal places, half-to-even**, returning the nearest f64
  to that decimal. It is NOT `(x*1e12).round()/1e12` — that is wrong in general.
  Implement via correctly-rounded decimal conversion (e.g. format the f64 to its
  exact/shortest decimal, round the decimal string at 12 places half-even, parse
  back). Verified: `round(1.0/61 + 0.5/70, 12) == 0.023536299766`.
- Rounding applies **only inside the sort key**; the stored fused value stays
  unrounded and is separately rounded to **6** places for evidence (§10).
- Tiebreak: `path` ascending, Python `str` comparison = Unicode code-point
  (scalar) order. Since paths are unique the key is total; sort stability
  is irrelevant.
- There is **no result limit** — every match is emitted.

## 10. Evidence object (`--explain` / MCP)

Always computed for every search match; emitted in JSON only when explaining.
Key order (dict insertion order — JSON must preserve it):

```json
"evidence": {
  "field": "id",              // winning tier name
  "terms": ["tier", "search"],// distinct casefolded query tokens, query order
  "tier": 0,                  // winning rank number
  "score": 0.024066,          // round(fused, 6)
  "components": {
    "bm25": 3.600463,         // round(bm25, 6)
    "lexical_rank": 2,        // int
    "graph_rank": 3,          // int
    "inbound": 14,            // int (the raw count, not the float)
    "graph_floor_ratio": 0.85,// fixed inclusive lexical floor
    "graph_gate": "applied"  // "applied" or "clamped"
  }
}
```

`round(_, 6)` = same Python decimal half-even rounding as §9 but 6 places.

## 11. `SearchResult` JSON — `rac find --json [--explain]`

`json.dumps(result.to_dict(include_evidence=explain), indent=2)` + trailing
`\n`. Defaults: `ensure_ascii=True` (non-ASCII → `\uXXXX` escapes, e.g. an em-dash
U+2014 in a title is emitted as the six ASCII bytes `\u2014` — verified), separators `(",", ": ")` with `indent=2`, insertion-order keys.
Floats render via Python `repr` (shortest round-trip; drops trailing zeros:
`round(1.3794195, 6)` → `1.37942`; switches to exponent form below 1e-4:
`1.5000000000000002e-05` — Rust's `{}` Display never emits exponent form, so a
custom Python-repr formatter is required).

Top-level shape:

```json
{
  "schema_version": "1",
  "query": "tier search",        // verbatim query string
  "type": null,                  // the --type value, "decision" under --decisions
  "match_count": 21,
  "matches": [ ... ]
}
```

Per-match key order: `id`, `type`, `title` (null when absent), `path`, then
conditionally `section` (only when not None), `snippet` (only when not None),
`evidence` (only with `--explain`), `recency` (always present on the CLI path —
§12), `tags` (only when the entry's tag list is **non-empty**; frontmatter
order). Metadata-tier matches omit `section`/`snippet` entirely (absent, not
null). Empty result: `"matches": []`, `match_count: 0`, exit 0.

## 12. Recency join (CLI-level, after ranking)

`cmd_find` calls `annotate_search_recency(result.matches, directory)` AFTER
ranking — matched set and order are never affected. Each match gains
`"recency": {"last_committed": <ISO-8601 with offset | null>, "age_days": <int | null>, "stale": <bool | null>}`
(all three keys always present; all null outside a git repository — verified).
Details (git boundary, threshold config, 180-day default) belong to the
recency section; for byte-parity note only that resolve never gets recency and
find always does (CLI path; the raw service result has `recency: None`).

## 13. `rac find` human output

No color codes when stdout is not a tty (`sys.stdout.isatty()` checked once at
import). Empty result: `No artifacts match {query!r}.` — `!r` is **Python
string repr**: single quotes by default; double quotes iff the string contains
`'` and no `"` (`"it's"`); if both quote kinds appear, single-quoted with `\'`
escapes; backslash and non-printables escaped Python-style. Rust must
reimplement Python `str.__repr__` exactly. Exit 0.

With matches:

```
RAC-KTXTAG63E89H               decision     ADR-038: Body-Text Search Tier
RAC-KTQ63DSJCAZ5               design       Explorer Command Surface
                                            ↳ Unified Search and Commands: Unified Search and Commands

96 match(es) for 'search'.
```

- `id_w = max(len(m.id))`, `type_w = max(len(m.type))` over matches (Python
  `len` = code points; `f"{v:<{w}}"` pads with spaces to code-point width).
- Row: `f"{id:<{id_w}}  {type:<{type_w}}  {title or '—'}"` (two spaces between
  columns; title NOT padded). A title shorter than the column produces trailing
  content only — but a padded id/type shorter than max gets trailing spaces
  *inside* the row (bytes matter).
- Stale marker appended to the row when `recency["stale"]` is truthy:
  `"  ⚠ stale ({age}d)"` (or `"  ⚠ stale"` if `age_days` is None), wrapped in
  `\x1b[33m…\x1b[0m` only on a tty. UNVERIFIED live (corpus is fresh); string
  taken from code.
- Snippet line (only when `snippet is not None`):
  `f"{indent}↳ {section}: {snippet}"` where `indent = " "*id_w + "  " + " "*type_w + "  "`;
  the `"{section}: "` prefix is omitted when `section` is falsy (empty string).
- `--explain` appends per match:
  `f"{indent}• field={field} terms={t1,t2}"` (terms comma-joined, no spaces);
  when the match has a snippet, ` [{section}: {snippet}]` is appended (section
  prefix again omitted when falsy); then a second line
  `f"{indent}  score={score} bm25={bm25} lexical_rank={lr} graph_rank={gr} inbound={inb} graph_floor_ratio={ratio} graph_gate={gate}"`
  — floats via `str()` = repr (`score=0.023272 bm25=1.37942`).
- Footer: empty line, then `f"{count} match(es) for {query!r}."`.
- Everything joined with `\n`, one trailing `\n` from `print`. Exit always 0.

## 14. CLI flags, cache, and exit codes (`find`)

`rac find <query> [directory=.] [--type T | --decisions] [--tag TAG]...
[--json] [--explain] [--cache/--no-cache] [--verify] [--top-level] [--recursive]`

- Directory guard: not a dir → stderr `rac: not a directory: {dir}\n`, exit 2.
- `--top-level` disables recursion; `--recursive` is a no-op affirmation.
- Cache (ADR-112): ON by default; `--no-cache` or non-empty `DECIDED_NO_CACHE`
  env selects the fresh walk. **Contract: cached and fresh output are
  byte-identical** (verified with `cmp` on `--json --explain` output). The Rust
  port can treat the fresh path as normative. `--verify` only affects cache
  freshness checking, never output bytes.
- Exit code is always 0 after the guard (empty result included);
  argparse errors (unknown flag, `--type` with `--decisions`) exit 2 with
  argparse's usage text on stderr.
- `rac find` never limits, paginates, or truncates.

## 15. Parity landmine checklist (floats & Python-isms)

1. **Summation order** — weighted_tf in `id,title,path,heading,body,tags` field
   order (tags LAST despite tier 2), score in query-token order; zero-tf fields
   skipped, not added as 0.0.
2. **Duplicate query tokens** inflate `df` AND double-add per-term score (§7.1).
3. **`round(x, 12)` / `round(x, 6)`** = decimal half-even correct rounding of
   the binary value; sort uses round(·,12) of fused, evidence uses round(·,6);
   the fused value itself is never mutated.
4. **`math.log(1 + x)`**, natural log, plain add — not log1p; libm `log`
   parity required (glibc vs musl last-ulp risk).
5. **Float equality in competition ranks** — a 1-ulp bm25 divergence changes
   rank sharing, then fused, then order.
6. **Python float repr** in JSON and human output (shortest round-trip,
   `1e-05`-style exponents, trailing-zero drop).
7. **Python `str` repr** (`{query!r}`) in human output, including quote-flipping.
8. **ASCII-only tokenizer** vs **full-Unicode casefold/strip** in resolve and
   the `--tag` facet — two different case regimes in one file.
9. **`json.dumps(..., indent=2)`** with `ensure_ascii=True` and insertion-order
   keys; conditional keys are *absent*, never null (except `title`/`type`,
   which are emitted as null).
10. Corpus stats are **corpus-global** (all types, unknowns included) even under
    `--type`/`--tag`; ranks are computed over the **matched set only**; the
    `--decisions` liveness drop happens **after** ranking.

## 16. UNVERIFIED

- Stale-marker rendering (`⚠ stale (Nd)`) and its yellow coloring — read from
  code; the live corpus had no stale matches. Build a fixture with an old
  commit date to lock the bytes.
- Tty-color variants (bold resolve id, yellow stale) — code-read only; the
  parity harness runs piped, where output is colorless.
- `--decisions` on the cached path via `ReadModelView.find_decisions`
  (index_store) — claimed byte-identical (ADR-104); the fresh
  `find_decisions_in` path was exercised, the store fast path only via default
  `rac find` (which compared identical). Cross-check once the store section
  lands.
- Behavior when two index entries share a `path` (dict-keyed stats would
  collapse them) — believed unreachable from a corpus walk.
- Exact argparse usage/error text for exit-2 paths (owned by the CLI section).
