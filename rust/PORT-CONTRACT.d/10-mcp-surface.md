# 10 — MCP surface: the `rac mcp` stdio wire contract

Scope: the JSON-RPC/MCP stdio surface of `rac mcp` — framing, handshake,
`tools/list` bytes, tool result shape, the ADR-033 budget as observed on the
wire, audit/telemetry side channels, and the parity rule for a two-server
harness. Source modules: `src/asdecided/mcp/{server,budget,errors,audit,telemetry,
transport,ping,surface}.py`, CLI wiring `src/asdecided/cli.py` (`cmd_mcp`,
`p_mcp`). HTTP transport (ADR-098) is out of scope here; stdio is the pinned
default and the porting target.

Two oracles were captured empirically (raw frames as bytes) over both the live
`rac/` corpus and the fixture corpus `tests/fixtures/mcp/corpus`:

- **PRIMARY** `.venv-oracle/bin/rac` — the 5-tool surface: `get_artifact`,
  `search_artifacts`, `find_decisions`, `get_related`, `get_summary`.
- **ORACLE-NEXT** (scratch worktree venv) — the 6-tool surface: adds
  `retrieve_grounding` (ADR-113), plus `live_only` on `search_artifacts` and a
  per-call `budget` argument on `get_artifact` / `retrieve_grounding`.

Both bundle the Python MCP SDK **`mcp==1.28.1`** and pydantic **2.13**. Every
claim below is verified from captured bytes unless marked UNVERIFIED.

---

## 0. Server invocation and environment neutralization

```
rac mcp --root <ROOT>            # stdio, cache on (ADR-112)
```

- `--root` defaults to `.`; a non-directory root exits via `_usage_error`
  before serving. The root string is used **verbatim**: artifact `path`
  fields and the summary `directory` field embed the resolved-from-`root`
  absolute paths as the walker produces them. A parity harness MUST pass the
  identical `--root` string (absolute path recommended) to both servers.
- Cache: on by default; `--no-cache` or `DECIDED_NO_CACHE=1` disables. **Verified:
  cache-on vs `DECIDED_NO_CACHE=1` runs are frame-for-frame byte-identical EXCEPT
  for duplicate-token queries** (see §0a — an oracle defect; the ADR-112
  guarantee holds on the wire for queries whose token list has no repeats).
  The cache lives under `$XDG_CACHE_HOME/rac/derived`. As a consequence, the
  parity harness pins `DECIDED_NO_CACHE=1` in the base environment of BOTH
  servers, making the no-cache engine path the canonical comparison path —
  the same path every other parity claim in this spike compares against.

### 0a. Oracle defect found — duplicate-token df divergence (ADR-112 violation)

**The Python oracle's cache-on serving is NOT byte-identical to its own
no-cache path when a query/task repeats a token** (e.g. `search_artifacts`
`{"query":"budget budget"}`). Verified on both oracles over the live corpus:
the cache-on `evidence` bytes differ from the no-cache bytes
(`components.bm25` 3.928816 cache-on vs 2.62216 no-cache on the top
`"budget budget"` match).

Root cause (from source): the two serving paths disagree on how a duplicated
term contributes to document frequency.

- No-cache (`src/asdecided/services/resolve.py`, `_corpus_stats`, ~lines 730-738):
  `df = dict.fromkeys(terms, 0)` then `for term in terms: ... df[term] += 1`
  per matching document — the loop runs once per *occurrence*, so a term
  listed twice increments its df twice per matching doc (df doubles, idf
  drops, bm25 drops).
- Cache-on (`src/asdecided/services/index_store.py`, ~lines 994 and 1047):
  `df = {term: self.prefix_df(term) for term in terms}` — a dict
  comprehension keyed by term, so a duplicated term is counted once from the
  persisted postings (dedup).

This violates ADR-112's invariant (cache on is byte-identical to the uncached
path) for this input class. It is an oracle bug, not a port target: **the
Rust engine matches the no-cache path** (as PORT-CONTRACT.d/06 pinned for the
CLI search surface), and the parity harness pins `DECIDED_NO_CACHE=1` on both
servers (§0 above) so the comparison referees the consistent engine path. The
duplicate-token cases in `rust/mcp-parity-cases.json`
(`search-duplicate-token-*`, `decisions-duplicate-token`,
`retrieve-duplicate-token-task`) hold this pin under test.
- Neutralization for deterministic runs (same posture as the CLI parity
  harness): point `HOME`, `XDG_STATE_HOME`, `XDG_CONFIG_HOME`,
  `XDG_CACHE_HOME` at scratch dirs. That guarantees: no consent record →
  no daily ping thread and no PostHog traffic (`ping.py` requires
  `$XDG_CONFIG_HOME/rac/telemetry.json` consent + key); no telemetry log
  (also requires the `--telemetry` flag, off by default); no audit recorder
  (requires an `audit:` stanza in the nearest `.decided/config.yaml` **at or
  above the root** — note the upward search in `audit._find_config_file`,
  so a corpus inside a repo that has `.decided/config.yaml` can pick it up).
- cwd: the server never depends on cwd for tool responses when `--root` is
  absolute (all reads go through `root`); run with cwd = repo root anyway —
  `_git_identity` and `artifact_provenance` invoke `git` with `cwd=root`.

### stderr is a diagnostic channel, never protocol

stdout carries only JSON-RPC frames (verified: first byte written is the
`initialize` response). stderr carries, in this order as applicable:

1. `rac mcp: no RAC artifacts found under '<root>'. …` (empty-corpus notice)
2. `rac mcp: telemetry on — …` (only with `--telemetry`)
3. `rac mcp: audit on — appending one line per read-tool call … to <path>`
4. `rac mcp: derived-index cache on (the default) — … under <cache_dir> …`
   (absent under `--no-cache`/`DECIDED_NO_CACHE`)
5. `rac mcp: anonymous usage sharing on — …` or the no-key variant (consent only)
6. Per-request SDK log lines: `Processing request of type ListToolsRequest` /
   `CallToolRequest`, and `Tool '<name>' not listed, no validation will be
   performed` before an unknown-tool call. (Python `logging` INFO/WARNING to
   stderr from the SDK — a Rust port does not need to reproduce these.)

stderr is **declared-normalized** in parity comparison (see §9).

---

## 1. Message framing

**Newline-delimited JSON. No Content-Length headers, ever.**

- One JSON-RPC message per line, terminated by a single `\n` (0x0A). Zero
  `\r` bytes anywhere in captured streams.
- Encoding UTF-8; **non-ASCII is emitted raw** (em-dashes in tool
  descriptions appear as UTF-8 bytes, not `\uXXXX`) — the SDK serializes
  envelopes with pydantic `model_dump_json` semantics (compact, non-ASCII
  preserved).
- The client writes requests the same way (the SDK parser is
  line-oriented). The server tolerates any valid JSON on one line.
- An unparseable input line does **not** kill the server and produces no
  response frame for that line; instead the server emits a logging
  notification (verified bytes):

  ```
  {"method":"notifications/message","params":{"level":"error","logger":"mcp.server.exception_handler","data":"Internal Server Error"},"jsonrpc":"2.0"}
  ```

  Note the field order on this SDK-generated notification: `method`,
  `params`, `jsonrpc` — different from response frames. Parity harnesses
  should not send garbage; if they do, this frame is what to expect.

## 2. Envelope serialization

SDK envelopes are **compact**: separators `,` and `:`, no spaces, keys in
model-field order, `null`-valued optional fields omitted. Response field
order is `jsonrpc`, `id`, `result` (or `error`). Verified examples below are
exact bytes.

The **inner tool payload** is a different serializer (see §5): Python
`json.dumps(payload, ensure_ascii=False)` with **default separators**
`", "` / `": "` — i.e. the tool-result JSON string has a space after every
colon and comma. (`budget._dumps`'s docstring says "no spaces"; the code
does not pass `separators`, so the wire truth is *with* spaces. Port the
code, not the comment.) `sort_keys` is never used; key order is dict
insertion order as pinned per tool in §5.

## 3. Handshake

Client sends `initialize`, server replies (exact bytes, both oracles
identical):

```
{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2025-06-18","capabilities":{"experimental":{},"prompts":{"listChanged":false},"resources":{"subscribe":false,"listChanged":false},"tools":{"listChanged":false}},"serverInfo":{"name":"lore","version":"1.28.1"}}}
```

- `serverInfo.name` is `"lore"` (`SERVER_NAME`, ADR-039).
- **Landmine:** `serverInfo.version` is `"1.28.1"` — the **Python `mcp` SDK
  package version**, not the rac version (FastMCP default). A byte-parity
  Rust port must emit the same literal string the oracle build emits;
  declare it as a pinned constant tied to the oracle's SDK version.
- `protocolVersion` echoes the client's requested version when the SDK
  supports it (verified: request `2024-11-05` → response `2024-11-05`;
  request `2025-06-18` → `2025-06-18`). Pin the harness client to one
  version (`2025-06-18` used for all captures here).
- No `instructions` field is emitted.
- Client then sends `notifications/initialized` (no response).
- `ping` → `{"jsonrpc":"2.0","id":N,"result":{}}`.
- `prompts/list` → `{"jsonrpc":"2.0","id":N,"result":{"prompts":[]}}`;
  `resources/list` analogous (capabilities advertise both with
  `listChanged:false` even though none exist). UNVERIFIED: `resources/list`
  exact bytes (inferred from `prompts/list` capture).
- Unknown method → JSON-RPC **error** frame:
  `{"jsonrpc":"2.0","id":N,"error":{"code":-32602,"message":"Invalid request parameters","data":""}}`

## 4. `tools/list` — the pinned, token-budgeted surface

One frame: `{"jsonrpc":"2.0","id":N,"result":{"tools":[…]}}`, no
`nextCursor`. Tools appear in **registration order**, not alphabetical:

- PRIMARY: `get_artifact`, `search_artifacts`, `find_decisions`,
  `get_related`, `get_summary` (frame is 4,727 chars + `\n`).
- ORACLE-NEXT: `get_artifact`, `search_artifacts`, **`retrieve_grounding`**,
  `find_decisions`, `get_related`, `get_summary`.

Each tool object has exactly the keys `name`, `description`, `inputSchema`,
`outputSchema`, in that order. Descriptions ship **character-for-character**
from the `DESC_*` constants in `server.py` (ADR-030 pins them; the corpus
gates hold the whole surface under a token budget —
`rac/mcp/surface.py`: `STANDING_BUDGET_TOKENS` 1000/hard-cap 1250 on
PRIMARY, 1350/1500 on ORACLE-NEXT for six tools).

Schemas are pydantic-generated from the Python signatures (the `ctx: Context`
parameter is excluded). Exact `inputSchema` bytes as served:

PRIMARY:

```
get_artifact     {"properties":{"id":{"title":"Id","type":"string"}},"required":["id"],"title":"get_artifactArguments","type":"object"}
search_artifacts {"properties":{"query":{"title":"Query","type":"string"},"type":{"anyOf":[{"type":"string"},{"type":"null"}],"default":null,"title":"Type"},"tags":{"anyOf":[{"items":{"type":"string"},"type":"array"},{"type":"null"}],"default":null,"title":"Tags"}},"required":["query"],"title":"search_artifactsArguments","type":"object"}
find_decisions   {"properties":{"topic":{"default":"","title":"Topic","type":"string"},"path":{"anyOf":[{"type":"string"},{"type":"null"}],"default":null,"title":"Path"}},"title":"find_decisions_toolArguments","type":"object"}
get_related      {"properties":{"id":{"title":"Id","type":"string"},"depth":{"default":1,"title":"Depth","type":"integer"}},"required":["id"],"title":"get_relatedArguments","type":"object"}
get_summary      {"properties":{},"title":"get_summaryArguments","type":"object"}
```

ORACLE-NEXT deltas:

```
get_artifact     adds "budget":{"default":0,"title":"Budget","type":"integer"} after "id"
search_artifacts adds "live_only":{"default":false,"title":"Live Only","type":"boolean"} after "tags"
retrieve_grounding {"properties":{"task":{"title":"Task","type":"string"},"scope":{"default":"","title":"Scope","type":"string"},"top_k":{"default":5,"title":"Top K","type":"integer"},"budget":{"default":0,"title":"Budget","type":"integer"},"live_only":{"default":true,"title":"Live Only","type":"boolean"}},"required":["task"],"title":"retrieve_grounding_toolArguments","type":"object"}
```

Every tool also serves an `outputSchema` (because the handlers return `str`):

```
{"properties":{"result":{"title":"Result","type":"string"}},"required":["result"],"title":"<fn>Output","type":"object"}
```

**Landmine — Python function-name leak:** schema `title`s come from the
*Python function* name, not the tool name: `find_decisions_toolArguments` /
`find_decisions_toolOutput` and `retrieve_grounding_toolArguments` /
`retrieve_grounding_toolOutput` (the handlers are `find_decisions_tool` and
`retrieve_grounding_tool`). A Rust port must hard-code these strings.
Also note pydantic conventions to replicate: `Title Case` property titles
derived from snake_case names (`"Top K"`, `"Live Only"`), `anyOf` +
`{"type":"null"}` + `"default":null` for `Optional[...]` params, key order
inside each property object is pydantic's (`anyOf`/`items` first, then
`default`, `title`, `type` — copy the pinned bytes above, do not re-derive).

## 5. `tools/call` result shape

Success (exact envelope, one line):

```
{"jsonrpc":"2.0","id":N,"result":{"content":[{"type":"text","text":"<PAYLOAD>"}],"structuredContent":{"result":"<PAYLOAD>"},"isError":false}}
```

- Exactly **one** content item, `{"type":"text","text":…}`, key order
  `type` then `text`. No annotations, no `_meta`.
- **`structuredContent` is present and duplicates the entire payload string**
  under key `"result"` (because the handler returns `str` and an
  outputSchema exists). The wire frame is therefore ~2× the payload plus
  JSON string escaping. `structuredContent.result` is byte-equal to
  `content[0].text` (verified on every captured success frame).
- `isError:false` is explicit on success.

`<PAYLOAD>` is the tool's JSON document serialized per §2 (spaces after
`:` and `,`, insertion-order keys, `ensure_ascii=False` so UTF-8 raw,
floats in Python `repr` form — scores like `0.024066`, `2.407751` are
pre-rounded to 6 decimals by core, but the port must match Python float
formatting generally).

### Payload key orders (pinned; from captured frames)

- `get_artifact`: `schema_version, id, type, title, path, content,
  provenance` (+ `truncated, omitted, hint` when truncated). `provenance`:
  `status, last_committed, last_author, first_committed, first_author,
  status_history` (git-derived; `null`/`[]` outside git).
- `search_artifacts`: `schema_version, query, type, match_count, matches`
  (+ markers). Match: `id, type, title, path, evidence, recency` (+ `tags`
  when tagged). `recency`: `last_committed, age_days, stale`.
- `find_decisions` topic mode: `schema_version, query, type, match_count,
  matches, filter` with `"filter": "live-decisions"` appended last; path
  mode: `schema_version, query, in_repository, decisions` (decision rows:
  `id, title, status, path, matching_entry`).
- `get_related`: `schema_version, id, type, title, path, outgoing, incoming`
  (+ `neighborhood, depth` when `depth>1`, + markers). `outgoing` is a dict
  section→raw target strings; `incoming` rows: `id, type, title, path,
  section, evidence{direction,relationship,target}`; `neighborhood` rows:
  `id, type, title, path, hops`.
- `get_summary`: `schema_version, directory, recursive, empty, artifacts,
  validation, completeness, relationships, attention, health,
  validation_status` (+ `guidance` when empty; + markers when over budget).
- `retrieve_grounding` (NEXT only): `schema_version, task, live_only, items`
  (+ `scope` handling per source, + markers). Item: `id, type, title,
  status, path, excerpt, provenance`.

### Tool-level errors (protocol-level, not ADR-034)

These come from the SDK, ride `isError:true`, and have **no
`structuredContent`**:

```
{"jsonrpc":"2.0","id":N,"result":{"content":[{"type":"text","text":"Unknown tool: no_such_tool"}],"isError":true}}
```

- Unknown tool → text `Unknown tool: <name>` (this is how PRIMARY answers a
  `retrieve_grounding` call: `Unknown tool: retrieve_grounding`).
- Invalid arguments → text is the **pydantic validation error**, verbatim,
  e.g. for `search_artifacts` with `{}`:

  ```
  Error executing tool search_artifacts: 1 validation error for search_artifactsArguments
  query
    Field required [type=missing, input_value={}, input_type=dict]
      For further information visit https://errors.pydantic.dev/2.13/v/missing
  ```

  **Landmine:** the message embeds the pydantic major.minor (`2.13`) in the
  URL. Byte parity on bad-argument paths ties the port to the oracle's
  pydantic version; pin these strings as constants.
- **Lenient argument handling (landmine):** unknown extra arguments are
  silently ignored (`get_summary {"bogus":1}` succeeds), and pydantic
  *coerces* types (`depth:"2"` string is accepted as 2). A strict Rust
  deserializer would reject both; the port must be equally lenient.

### Structured lookup errors (ADR-034 — data, not protocol)

Failed lookups are **successful** tool calls (`isError:false`, with
`structuredContent`) whose payload is the error document. Exact payload
bytes:

```
{"schema_version": "1", "error": "not-found", "id": "RAC-DOESNOTEXIST"}
{"schema_version": "1", "error": "duplicate", "id": "<id>", "paths": [...]}     (from source; UNVERIFIED bytes)
{"schema_version": "1", "error": "unreadable", "id": "<id>", "path": "<path>"}  (from source; UNVERIFIED bytes)
{"schema_version": "1", "error": "audit-unavailable", "tool": "<tool>"}         (audit block mode; from source; UNVERIFIED bytes)
```

`not-found`/`duplicate` are `ResolutionResult.to_dict()` — identical to
`rac resolve --json` bodies (section 06 of this contract).

## 6. The ADR-033 budget on the wire

- Default budget **10,000 characters**, measured over the serialized
  **payload string** (`content[0].text`), *not* the wire frame (which is
  ~2× + escaping). The native server accepts `--budget N` on both stdio and
  HTTP; `N` must be at least **128**. The default remains 10,000 when the
  flag is omitted.
- ORACLE-NEXT per-call `budget` arguments may only lower the configured
  server budget: `effective = server if arg<=0 else min(server, arg)`. A
  positive per-call value below 128 is a tool error, never an oversized
  successful response.
- Truncation is deterministic and whole-item wherever a repeated collection
  is involved. Marker fields are appended (insertion order puts them last):
  `"truncated": true`, `"omitted": <int>`, `"hint": "<pinned string>"`.
  `truncated` is **absent** (never `false`) on complete responses. Pinned
  hints: `HINT_SEARCH`, `HINT_RELATED`, `HINT_CONTENT`, `HINT_SUMMARY`, and
  `HINT_RETRIEVE`.
- The serializer applies stable shape rules: `matches`, `incoming`,
  `neighborhood`, `items`, `decisions`, `attention`, and outgoing relationship
  targets are reduced from the tail as whole entries. Retrieve `items` may
  first shorten the last kept `excerpt`; artifact `content` may be shortened
  to the largest fitting prefix. Omitted counts include source overflow and
  every dropped item; a pure excerpt/content reduction reports dropped
  characters where the source shape supports it.
- **Every successful payload string is at or below the configured budget.**
  If fixed fields alone cannot fit, the server returns the small structured
  error `{"error":"response_budget_exceeded", "hint":"..."}` (or its
  shorter form at unusually small direct serializer limits), rather than an
  oversized success. This replaces the historical summary and deep-
  neighborhood overrun exceptions; the native engine does not preserve those
  bugs for compatibility.
- The budget serializer re-measures with `_dumps` (spaces included) — the
  10,000 counts those spaces.

## 7. Audit (ADR-084), telemetry (ADR-040), ping (ADR-041)

- **None of the three ever changes wire bytes.** Verified: identical call
  sequence with audit on vs off is frame-for-frame byte-identical (the one
  designed exception is audit `on_write_error: block` on a failed write →
  the `audit-unavailable` payload above).
- Audit: enabled only by an `audit:` stanza (`enabled: true`, optional
  `path`, `on_write_error: warn|block`) in the nearest `.decided/config.yaml`
  at/above the root; a malformed stanza refuses startup via `_usage_error`.
  Path resolution: `DECIDED_AUDIT_PATH` env > config `path` >
  `$XDG_STATE_HOME/rac/audit.jsonl`. One JSON line appended per tool call,
  `json.dumps(…, ensure_ascii=False)` **default separators (spaces)**, key
  order: `schema_version, ts, session, principal, transport, attribution,
  tool, query, returned, outcome, duration_ms`. Example captured:

  ```
  {"schema_version": "1", "ts": "2026-07-11T22:54:50.988Z", "session": "1ad79b7d1ab1deae", "principal": "unattributed", "transport": "stdio", "attribution": "local", "tool": "get_summary", "query": {}, "returned": [], "outcome": "ok", "duration_ms": 33}
  ```

  Nondeterministic per run: `ts` (UTC, milliseconds, `+00:00`→`Z`),
  `session` (`secrets.token_hex(8)` per process), `duration_ms`.
  `principal`: `DECIDED_AUDIT_PRINCIPAL` > `git config user.name/user.email`
  **run in the root dir** > `"unattributed"`. `query` echoes the audit args
  the wrapper builds (optional args ride only when supplied); `returned` is
  the deduped, body-free artifact-reference list parsed back out of the payload.
  Every reference carries `id`, `resolved`, and `provenance.path`; the parser
  considers the primary artifact plus `matches`, `items`, `decisions`, `incoming`,
  and `neighborhood` collections and never records artifact bodies. The audit **file** is
  a side artifact — compare it structurally if at all, never byte-wise.
- Telemetry: requires `--telemetry`; JSONL at
  `$XDG_STATE_HOME/rac/guide-telemetry.jsonl`; content-free; never wire-
  visible; self-disables on first write failure.
- Ping: requires consent file + compiled-in PostHog key; daemon thread;
  neutralized entirely by scratch `XDG_CONFIG_HOME`. A Rust port that never
  implements it stays wire-identical.

## 8. Determinism — the answer

**Yes.** Running the identical 15-call sequence twice against the same
server (same root, same scratch XDG dirs, same day) produced **byte-identical
stdout streams** for PRIMARY and for ORACLE-NEXT (and cache vs no-cache is
also byte-identical). Nothing clock-, pid-, or session-derived reaches
stdout. The nondeterminism lives only in side channels and inputs:

| Source | Where it shows | Parity handling |
| --- | --- | --- |
| `secrets.token_hex` session ids | audit/telemetry files only | ignore files |
| `ts` / `duration_ms` | audit/telemetry files only | ignore files |
| git state of the checkout | `provenance`, `recency.last_committed`, `status_history`, incoming graph | **control**: run both servers over the same commit of the same checkout |
| wall clock (day granularity) | `recency.age_days` / `recency.stale` (now − commit date) | control: same-day runs; flag day-boundary risk in long harnesses |
| `--root` string | every `path`, summary `directory` | control: identical absolute root |
| git identity / `DECIDED_AUDIT_PRINCIPAL` | audit file only | ignore files |
| SDK/pydantic versions | `serverInfo.version`, validation-error text | declare: pinned to `mcp 1.28.1` / pydantic `2.13` |

## 9. Parity comparison rule (two-server harness)

1. **Byte-compare (MUST): `content[0].text` of every `tools/call` result.**
   This is the product contract — ADR-032/ADR-033 promise byte-identical
   payloads for identical corpus bytes, and the oracle delivers it (all five
   shared tools were byte-identical across PRIMARY and NEXT). It is also
   what the agent actually reads. Comparing the payload string (not the
   parsed JSON) pins separators, key order, float formatting, and
   truncation boundaries for free.
2. **Byte-compare (SHOULD): the full response line for `tools/call` and
   `tools/list`.** Justified because the envelope is deterministic and the
   Rust port must reproduce `structuredContent` duplication, `isError`
   placement, and compact envelope separators. If the port uses a different
   (spec-compliant) envelope serializer, downgrade to: parse envelope
   structurally, assert `structuredContent.result == content[0].text`,
   `isError`, single text item — and still byte-compare the inner text.
3. **Structural compare: `initialize` result.** Assert `serverInfo.name ==
   "lore"` and the capabilities object shape; `serverInfo.version` is
   **declared-normalized** (SDK version string, not a product value —
   byte-pinning it would weld the harness to the Python SDK release).
   Protocol-version echo must match the harness's fixed request.
4. **Declared-normalized (never compared): stderr, audit/telemetry/ping
   files, cache directory contents, SDK log notifications, response
   latency.** These are observability and infrastructure, explicitly outside
   the request/response contract (ADR-032/040/084); the recorded guarantee
   is only that they don't perturb stdout — which the harness re-proves by
   running audit-on once and byte-comparing against audit-off.
5. **Error paths compare byte-wise too**, with one caveat: pydantic
   validation text and `Unknown tool: …` strings are SDK-owned; treat them
   as pinned constants of this contract (they already differ from ADR-034
   structured errors, which are ordinary payloads under rule 1).

## 10. PRIMARY vs ORACLE-NEXT — measured deltas

For the identical 15-call live-corpus sequence and 8-call fixture sequence,
**every frame was byte-identical except**: (a) `tools/list` (sixth tool +
`budget`/`live_only` schema additions), and (b) the `retrieve_grounding`
call (PRIMARY: `Unknown tool` error; NEXT: the grounding payload). The five
shared tools' payloads — including truncation boundaries, over-budget
summary/related shapes, and structured errors — did not differ by one byte.
So the six-tool port target is a strict superset: implement the 5-tool
contract, then add `retrieve_grounding`, the `items` budget rule, the
`live_only` search facet, and the per-call `budget` clamp.

## 11. Harness reproduction

Capture driver: `scratchpad/mcpwire/drive.py` (spawns `<rac> mcp --root …`
with scratch XDG env, cwd = repo root; writes NDJSON requests; saves raw
stdout frames). Call baskets: `calls-live.json` (tools/list; get_summary;
2× search; get_artifact ADR-001; get_related depth 1 and 3; find_decisions
topic + path modes; >10k get_artifact `RAC-KVSF2ZC1BFC5`; not-found id;
unknown tool; missing required arg; retrieve_grounding) and
`calls-fixture.json` over `tests/fixtures/mcp/corpus`
(`RAC-MCPDEC000001` et al.). Client `initialize` pinned to
`protocolVersion: 2025-06-18`, `clientInfo {"name":"port-parity-harness",
"version":"0.0.1"}`.
