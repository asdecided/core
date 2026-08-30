# Index store on-disk format (ADR-104 / ADR-106 / ADR-112)

Durable byte-level specification of the persistent derived-index store. The
v1 format was extracted from the frozen Python oracle; ADR-143 cuts the native
engine over to a source-aware v2 layout. Unchanged segment families retain
their byte vectors, while v2 entries, relationships, liveness, scope, layers,
and composite maps carry federation identity. Old `store/v1` directories are cache misses
and are never decoded as v2. Everything below is deterministic — no
timestamps, no pids, no floats on disk (temp-file *names* embed pid/random
bytes but never survive the atomic rename).

## 1. Cache directory layout

```
<cache_dir>/                              # default_cache_dir(), §8
  <corpus_hash>.json                      # marker (schema gate), §7
  store/v2/<generation_hash>/             # one store dir per logical generation
    header.seg entries.seg identities.seg sections.seg tokens.seg termdict.seg
    postings.seg relationships.seg live.seg scope.seg portfolio.seg
    aliasmap.seg pathmap.seg keymap.seg artifactpathmap.seg layers.seg
    redirects.seg                           # exactly these 17 files
  validate/v1/<root_key>.vseg             # per-root validation rows, §9
  manifest/v1/<root_key>.fseg             # per-root stat manifest, §10
```

- `STORE_DIRNAME = "store"`, `STORE_LAYOUT_VERSION = "v2"`.
- `generation_hash` is the single-corpus content hash (§6) when there is no
  federation manifest, or the framed composed-generation hash (§6.1).
- Writes are atomic: segments land in `store/v2/.<hash>.tmp-<pid>-<hex8>`,
  each file `fsync`ed, the dir fsynced (best-effort), then `os.replace`d
  onto the final name. `.vseg`/`.fseg` writes use the same temp+replace
  shape with one file.
- A pre-existing store dir for the same hash is probed with a full reader
  open; if openable it is kept (content addressing — byte-equivalent),
  otherwise removed and replaced.

## 2. Segment framing (`index_format.py`)

Every `*.seg`, `*.vseg`, `*.fseg` file is one framed segment:

```
magic     8 bytes   b"RACIDX01"
version   u16 LE    SEGMENT_FORMAT_VERSION = 4
plen      u64 LE    payload length in bytes
payload   plen bytes
```

Open gate (fail closed, O(1)): file at least 18 bytes; magic exact;
version exactly 4; file length exactly `18 + plen` (short = truncated,
long = trailing garbage; both are misses). An empty (0-byte) segment file
fails before framing ("empty segment").

### 2.1 Primitive encodings (all little-endian)

- `u32` — 4 bytes; writer range-checks `0 <= v <= 0xFFFFFFFF` (raises
  IndexFormatError, becomes "not written").
- `u64` — 8 bytes (Python packs unchecked; values are sizes/offsets).
- `blob` — `u32 len` + raw bytes.
- `text` — `blob` of UTF-8.
- `opt_text` — 1 flag byte: `0x00` = None; `0x01` + `text` = present.
  Any other flag byte is a decode error. None ≠ empty string.
- `text_list` — `u32 count` + count × `text`.
- `u32_list` — `u32 count` + count × `u32`.

### 2.2 Indexed segments (`write_indexed`)

Row-addressable payload for O(1) point access:

```
count    u32
offsets  count × u64      # each row's offset relative to end of table
rows     concatenated row blobs (no per-row length; offsets delimit)
```

Reader gates: offset table must fit the payload; `row(k)` requires
`0 <= k < count` and `data_start + offset <= len`.

## 3. The 17 read-model segments

Search docids are assigned in effective `index_entries` order. Identity
docids are assigned independently in the central composition layer's exact
point-resolution order. In a non-federated corpus both projections retain the
corpus walk's sorted-path order (`sorted(Path)` over
`find_markdown_files`, §6).

Field order is a parity contract:
`FIELDS = ("id", "title", "path", "heading", "body", "tags")`.

- **entries.seg** (indexed, one row per doc):
  `text id | text type | opt_text title | text path | text_list aliases |
  text_list tags | u32 inbound_count | 6 × u32 per-field token counts |
  opt ArtifactKey | opt ArtifactPath | opt ArtifactOrigin`
  (field lengths in FIELDS order, `len(field_tokens[name])`).
  Each `opt` uses a `u32` 0/1 presence flag. `ArtifactKey` is
  `text source | text canonical_id`; `ArtifactPath` is
  `text source | text relative_path`; `ArtifactOrigin` is
  `text source | text layer | opt_text pin | opt_text alias`, where layer is
  exactly `local` or `inherited`. Fresh v2 rows always carry all three values;
  absence remains decodable only for compatibility fixtures.
- **identities.seg** (indexed, one row per authorized point-resolution row):
  `text id | text type | opt_text title | text path | text_list aliases |
  text_list tags | opt ArtifactKey | opt ArtifactPath | opt ArtifactOrigin`.
  It may retain an overridden parent under its qualified canonical alias while
  attaching that parent's unqualified canonical id to the local replacement.
  It deliberately has no search sections, inbound count, or field lengths.
- **sections.seg** (indexed): `u32 nsections` then per section
  `text heading | text_list lines`.
- **tokens.seg** (indexed): 6 × `u32_list` of term ids, FIELDS order,
  document token order preserved.
- **termdict.seg** (indexed, one row per term): `text term`. Terms are
  the sorted (Python `sorted`, i.e. code-point order on str) global
  vocabulary of all six fields of all docs; term id = row index.
- **postings.seg** (indexed, one row per term id): `u32_list docids` —
  every doc holding that term in ANY field, ascending by construction
  (appended in docid order, deduped per doc via the doc's distinct term
  id set).
- **aliasmap.seg** (indexed): rows sorted by casefolded key (Python
  `str.casefold`, code-point sort); row = `text key | u32_list docids`
  (ascending, consecutive-duplicate-guarded). Keys and docids refer to
  `identities.seg`, so qualified parent lookup and override redirects survive
  a warm open exactly as composed.
- **pathmap.seg** (indexed): rows sorted by path *string* (not Path
  order); row = `text path | u32 docid`.
- **keymap.seg** (indexed): rows sorted by `(source, canonical_id)`; row =
  `text source | text canonical_id | u32_list identity_docids`. Multiple
  docids retain a deterministic ambiguity instead of acquiring precedence.
- **artifactpathmap.seg** (indexed): rows sorted by
  `(source, relative_path)`; row =
  `text source | text relative_path | u32 identity_docid`.
- **relationships.seg** (plain): `u32 count` then per relationship
  `text source_path | text relationship | text target |
  opt_text resolved_path | opt_text issue | opt ArtifactPath source_artifact |
  opt ArtifactPath resolved_artifact`.
- **layers.seg** (plain): `u32 count` then per stable layer
  `text source | text layer | opt_text pin | opt_text alias`.
- **redirects.seg** (plain): `u32 count` then per validated mapping three
  required ArtifactKeys in order: parent, replacement, rationale. Each key is
  encoded with the standard `u32 present=1 | text source | text canonical_id`
  shape. Raw manifest YAML is never persisted as resolution authority.
- **live.seg** (plain): `u32 count` then per live decision
  `required ArtifactKey | text display_path`. The key is the filtering
  authority; the display path is retained for released single-corpus output.
- **scope.seg** (plain): `u32 count` then per row
  `opt ArtifactKey | opt ArtifactPath | opt ArtifactOrigin |
  text id | text title | text status | text path |
  text_list scope_entries`.
- **portfolio.seg** (plain): one `text` — the portfolio summary dict as
  `json.dumps(obj, ensure_ascii=False)` (compact-with-spaces default
  separators `", "`/`": "`, insertion-ordered keys). The ONLY
  JSON-in-binary blob.
- **header.seg** (plain, written last):
  `text corpus_hash | text bundle_version | text scoring_fingerprint |
  u32 n_entries | 6 × u32 field length sums (FIELDS order) | u32 n_terms`.

### 3.1 Header gates on open

After framing checks on all 17 segments, the reader reads header.seg and
fails closed on: stored hash ≠ requested corpus hash; stored bundle ≠
`SCHEMA_VERSION = "3"`; stored fingerprint ≠ the compiled-in
`scoring_fingerprint()`. Fingerprint string (pinned):

```
id=4.0|title=3.0|path=2.0|heading=1.5|body=1.0|tags=2.5|k1=1.2|b=0.75|rrf=60|graph=0.5|graph_floor=0.85
```

Float repr rule: Python `repr()` — `4.0`, `1.5`, `0.75`, `1.2`; ints
stay ints (`rrf=60`). The Rust side must emit this exact string.

## 4. Determinism notes (why byte-identity holds)

- Docid order = walk order = `sorted(list[Path])`. Path ordering for
  relative paths of the form `a/b/c.md` is componentwise string
  comparison; the Rust walk (`walk.rs`) already reproduces it.
- termdict order = Python `sorted` over str = Unicode code-point order.
- aliasmap key order = code-point order of casefolded keys. Casefold is
  full Unicode casefolding (e.g. `ß → ss`, `İ → i̇`); the engine's
  existing `pycompat` casefold is the reference.
- postings rows and aliasmap docid lists are ascending by construction.
- pathmap order = code-point order of the path strings.
- portfolio JSON: `ensure_ascii=False`, default separators, dict
  insertion order (the portfolio composer's construction order).
- No floats are ever written; sums are u32 integer accumulators.

## 5. Failure = miss, never an answer change

Every reader-side failure (missing dir/file, empty file, bad magic,
version, truncation, header mismatch, u32 range, bad opt flag, UTF-8
error) raises IndexFormatError-or-OSError and is caught at
`open_read_model` → `None` → fresh build. Every writer-side failure
degrades to "not written" (fresh structures already in hand). The cache
can only change latency.

## 6. Corpus hash

Per file: `content_hash = sha256(file bytes).hexdigest()`; unreadable
files hash the sentinel `sha256(b"\x00rac-unreadable-artifact")`.
Corpus: over `find_markdown_files(dir, recursive)` in sorted order,
fold `rel_posix_path utf8 | \0 | ascii hexdigest | \0` into one sha256;
hexdigest is the corpus hash. `corpus_hash_from_manifest` reproduces
this from cached per-file hashes (re-hashing any file absent from the
manifest) — byte-identical for every non-S5 state.

### 6.1 Federated logical generation hash

A repository with a verified parent uses domain
`asdecided-federated-generation-v1\0` and one-byte-tag + u64-big-endian-length
frames over: the hash of the exact captured child path/byte snapshot; child
source; exact child config bytes; exact manifest bytes; parent source;
verified full parent digest; exact parent config bytes; parent alias; an
explicitly presence-tagged override-mapping payload; the `local` / `inherited`
layer labels; the recursive/top-level flag; and the child-repository-relative
corpus root. Parent verification and child capture precede key construction
and every cache reuse. The central composer consumes those captured child
bytes and the verified parent's captured bytes without a second walk or read.
Changing any child corpus/config/mode/root, manifest, alias, override, parent
config/source, parent artifact, or pin input therefore selects a different
generation. Checkout paths never enter the key or a public artifact path. A
repository without the manifest continues to use the corpus hash above
unchanged.

## 7. Marker file

`<cache_dir>/<corpus_hash>.json` — written AFTER the store lands
(marker present ⇒ store present):
`json.dumps({"schema_version": "3", "corpus_hash": corpus_hash})`
(compact-with-spaces separators, ensure_ascii default True — content is
ASCII), no trailing newline. Gate on read: parse, must be a dict with
`schema_version == "3"`; anything else is a miss. Written atomically
(NamedTemporaryFile + replace).

## 8. Cache directory resolution ladder

`DECIDED_CACHE_DIR` (non-empty) → `$XDG_CACHE_HOME/rac/derived` →
`~/.cache/rac/derived` → `<system tmp>/rac-cache/rac/derived` (homeless
floor). `--no-cache` or non-empty `DECIDED_NO_CACHE` disables the cache
entirely (`args.cache and not os.environ.get("DECIDED_NO_CACHE")`).

## 9. Validation-result store (`.vseg`, ADR-106)

Path: `validate/v1/<root_key>.vseg` where `root_key` is the manifest
root key (§10) of the validated root+mode. One framed segment:

```
text config_hash
u32  count
per row:
  text rel_path | u64 size | u64 mtime_ns | text content_hash |
  text artifact_type | text status |
  u32 issue_count × (text severity | text code | text message |
                     u32 has_line | u32 line_value)
```

`has_line`/`line_value` are `0,0` for a None line, `1,line` otherwise.
Row order: insertion order of the rows dict (the assembly's walk
order). A config-hash mismatch on open is a miss (full recompute).
Issues are path-free; the current path is re-attached at assembly, so a
rename with identical bytes reuses the row.

## 10. Freshness manifest (`.fseg`, ADR-112)

Path: `manifest/v1/<root_key>.fseg`;
`root_key = sha256("<resolved root>\0<mode>")` where mode is
`recursive` or `top-level` and the root is `Path(directory).resolve()`.
One framed segment:

```
u32 manifest_format_version = 1
u32 count
per row: text rel_path | u64 size | u64 mtime_ns | text content_hash
```

Row order: insertion order = the stat-scan's enumeration order
(`find_markdown_files` sorted order). The stat proxy `(size, mtime_ns)`
gates re-reads; `content_hash` is the truth. The S5 state (an in-place
rewrite preserving both size and mtime_ns) is the oracle's accepted
miss — pinned, not fixed; `--verify` (content-confirm-all) is the floor
that catches it.

## 11. Known oracle behaviors pinned as-is

- **S5 accepted miss** (§10). The stat rung reuses the manifest hash
  when `(size, mtime_ns)` are unchanged even if bytes differ.
- **Duplicate-token df divergence** (PORT-CONTRACT.d/10 §0a). The
  oracle's store-served search dedups a repeated query term's df where
  the walk counts per occurrence. The native engine keeps its pinned
  no-cache scoring on BOTH paths (warm == cold byte-neutrality wins
  over oracle-defect fidelity); duplicate-token queries are excluded
  from cache-on vs oracle comparisons and covered by native
  warm==cold tests instead.
- **`rac index` / `rac resolve` never consume the cache**; only `find`,
  `validate`, and the MCP server do. `rac index` may *write* nothing —
  it is a plain walk.
- **DECIDED_TIMING** writes stderr only; never a parity surface.
