# 14 — Closure state reporting: usage, mcp-stats, telemetry

Scope: the B3 local-state commands ported for
roadmap:native-cli-closure — `rac usage`, `rac mcp-stats`,
`rac telemetry` — plus the cross-cutting ADR-046 usage recorder they
imply. Every claim below was verified against the oracle
(`.venv-oracle/bin/rac`, `0.1.dev50+g21c8be403`, Python 3.11.15).
Source files: `src/asdecided/cli.py` (`cmd_usage`/`cmd_mcp_stats`/
`cmd_telemetry`, `main`), `src/asdecided/usage.py`, `src/asdecided/consent.py`,
`src/asdecided/mcp/telemetry.py`, `src/asdecided/output/{human,json}.py`
(`render_mcp_stats_*`). Rust: new `rac-engine/src/consent.rs`,
`usage.rs`, `telemetry.rs`; `output.rs` (`render_mcp_stats_*`,
`render_usage_*`); `commands.rs` (`cmd_mcp_stats`/`cmd_usage`/
`cmd_telemetry`); `cli.rs` (`parse_json_share_group`, `run_telemetry`,
the order-aware pre-scan exemption, the recorder wrapper);
`pycompat.rs` (`quote_plus`, `quote_plus_urlencode`).

These are the first ported commands whose inputs are LOCAL STATE, not
the corpus (ADR-040/041/046; ADR-086 hard-lock):

| file | dir | read by | written by |
| --- | --- | --- | --- |
| `rac/telemetry.json` | `$XDG_CONFIG_HOME` (else `~/.config`) | telemetry status/on; every recorder gate | `telemetry on/off [--enterprise [--unlock]]` |
| `rac/rac-usage.jsonl` | `$XDG_STATE_HOME` (else `~/.local/state`) | usage | the post-dispatch recorder, consent-gated |
| `rac/guide-telemetry.jsonl` | `$XDG_STATE_HOME` (else `~/.local/state`) | mcp-stats, usage | Guide MCP serving only (not these commands) |

Path building is `os.environ.get(VAR) or default` — an EMPTY env value
falls back (Python `or`), and the joined path is PurePosixPath-
normalized but NEVER resolved: a relative `XDG_STATE_HOME` reads
relative to the cwd and prints verbatim (`Log: rel-state/rac/...`),
which the share-URL parity cases exploit (see §5).

---

## 1. `rac usage [--json | --share]`

### 1.1 Argv surface
No positional; `--json|--share` is a mutually-exclusive group. Exit 0
for every log state (empty/missing logs are valid answers, no consent
gate on reads); exit 2 for the mutex conflict (`rac usage: error:
argument --share: not allowed with argument --json`, order-sensitive
message) and for extras (top-level `unrecognized arguments`).

### 1.2 Read semantics (`usage.summarize_usage` + guide `summarize`)
CLI log lines: `str.strip()`-blank lines skipped; non-JSON and
JSON-non-dict lines skipped WITHOUT counting (unlike the guide log).
`total` = all dict events; `sessions` = distinct string `session`
values; `commands` = events grouped by string `command`, sorted
(code-point order), `errors` counts `outcome in {"error",
"exception"}`; `recent` buckets `ts[:10]` (CODE-POINT slice, only when
`len(ts) >= 10`) into a Counter, then `sorted(items)[-7:]` — ascending
date order, trailing 7-day window. The guide half is exactly
mcp-stats' summary (§2.2), path included.

### 1.3 Output
- HUMAN: `RAC usage` / blank; cli-total==0 → `No CLI usage recorded —
  telemetry is off (enable with \`rac telemetry on\`).`, else
  `CLI commands: {total} calls across {sessions} session(s)` (always
  plural-shaped), rows `  {command:<16} {calls}` + `  ({n} error{'s' if
  n != 1 else ''})` only when n>0 — note the TWO leading spaces inside
  the suffix and the singular/plural flip; then `  recent: {d}: {n},
  ...` iff nonempty. Guide section iff guide `tools` nonempty: blank,
  `Guide MCP tools:`, rows `  {tool:<16} {calls}` + `  ({n} error(s))`
  (ALWAYS the `(s)` form — a different pluralization than the CLI rows,
  by oracle design). Column pad is `str.ljust`-style code points.
- JSON: `json.dumps(_combined, ensure_ascii=False, indent=2)` — raw
  UTF-8 (contrast mcp-stats §2.3). Shape: `{schema_version, cli:
  {schema_version,total,sessions,commands[],recent{}}, guide:
  <full guide to_dict>}`. The guide dict is always full (never `{}`),
  including nulls for `first_ts`/`last_ts` when empty.
- SHARE: one line, `https://github.com/itsthelore/asdecided-core/issues/new?
  template=guide-usage-report.yml&report=<quote_plus(combined JSON)>`.
  The report INCLUDES `guide.path` — usage does not strip the path;
  mcp-stats does (§2.3). Do not copy the strip across.

## 2. `rac mcp-stats [--json | --share]`

### 2.1 Argv surface
Identical shape to usage (shared `parse_json_share_group` in cli.rs).

### 2.2 Read semantics (`mcp.telemetry.read_events`/`summarize`)
Guide-only. Skipped-line counting: non-JSON lines AND JSON-non-dict
lines each +1; blank (`strip()`-empty) lines silent. `session_count` =
distinct string sessions; `first_ts`/`last_ts` = min/max of the SORTED
string timestamps (code-point sort, string ts only); per-tool rows
sorted by tool name: `calls`, `errors` (`outcome in {"error",
"exception"}`), `truncated` (`is True` exactly — a string `"yes"` does
not count), `avg_duration_ms` = `round(mean)` over durations where
`isinstance(d, int)` — CPython counts BOOLS as ints (`true` averages
as 1) and floats never; empty → 0; `round()` is half-to-even
(`pycompat::py_round`: 1.5→2, 2.5→2, pinned by `mcp-stats-edge-json`).

### 2.3 Output
- HUMAN: `Guide Telemetry` / `===============` / blank / `Log: <path>`;
  empty → blank, `No telemetry recorded.`, `Telemetry is off by
  default; enable it with: rac mcp --telemetry`; else `Events:` /
  `Sessions:` / `First Event:` / `Last Event:` (a populated log with no
  string ts prints `None`), blank, `Tool Usage` / `==========` / blank,
  rows `  {tool}: {c} call(s), {e} error(s), {t} truncated, avg {a} ms`.
  Either branch appends blank + `Skipped Unreadable Lines: {n}` iff
  n>0. Headers are `bold()` — byte-invisible under the piped harness.
- JSON: `json.dumps(to_dict(), indent=2)` — `ensure_ascii=True`, the
  ONLY ascii-escaped payload of these surfaces (`café`), pinned
  against usage's raw-UTF-8 by the unicode cases.
- SHARE: as §1.3 but the report DELETES `path` before dumping
  (counts + timestamps only), dumped `ensure_ascii=False, indent=2`.

### 2.4 quote_plus (both share URLs)
`urllib.parse.urlencode` ⇒ `quote_plus(value, safe='')` per value:
ALWAYS-SAFE set is alnum + `_.-~` only; space→`+`; every other UTF-8
byte → uppercase `%XX` (`:`→`%3A`, `{`→`%7B`, newline→`%0A`,
`é`→`%C3%A9`). `pycompat::quote_plus`; byte-pinned raw (no masks) by
the six share cases.

## 3. `rac telemetry [on|off|status] [--enterprise] [--unlock]`

### 3.1 Argv surface — ordering is the landmine
`action` is an OPTIONAL positional with choices `{on,off,status}`,
default `status`. argparse validates the choice when the positional is
CONSUMED but fires `--version`/`-h` actions at ENCOUNTER, and defers
unknown-token errors to end-of-parse. Measured consequences, all
pinned: `telemetry bogus --version` → exit 2 (invalid choice wins);
`telemetry --version bogus` → version, exit 0; `telemetry on off
--version` → version, exit 0 (extras deferred); `telemetry on off` →
top-level `unrecognized arguments: off`, exit 2; `usage --json --share
--version` → exit 2 (mutex fires at encounter). These three commands
are therefore EXEMPT from cli.rs's generic `--version`/`-h` pre-scan
and parse order-aware. `--` makes everything after it positional
(`telemetry -- on` opts in; `telemetry -- --enterprise` is an invalid
CHOICE, exit 2).

### 3.2 cmd validation order (three distinct exit-2 paths, stdout empty)
1. `--enterprise`/`--unlock` with action ≠ `off` → stderr `rac:
   --enterprise/--unlock are only valid with 'rac telemetry off'`
   (also hit by bare `telemetry --enterprise`, whose action defaults
   to status);
2. `--unlock` without `--enterprise` → `rac: --unlock requires
   --enterprise (use 'rac telemetry off --enterprise --unlock')`;
3. `on` while `enterprise_locked` → `rac: cannot opt in while the
   enterprise telemetry lock is set; remove it with 'rac telemetry off
   --enterprise --unlock' first (ADR-086).` — refusal leaves the file
   untouched (pinned by captured-file compare).

### 3.3 Consent record semantics (`consent.py`)
`load_consent`: missing/unreadable/non-JSON/non-dict → default
no-consent; non-UTF-8 is a `UnicodeDecodeError` = `ValueError` →
ALSO the tolerant default (unlike the logs, §6). Field coercions are
CPython's, applied per PRESENT key: `bool(value)` for the flags
(`"no"` → True — string truthiness), `str(value)` for the ids
(`null` → `"None"`, `42` → `"42"`, floats via repr, containers via
Python repr) — pinned by `telemetry-status-weird-types`.
`opt_in` mints `secrets.token_hex(16)` install_id/salt ONLY where the
existing value is falsy, ALWAYS re-mints `consented_at`
(`isoformat(timespec="seconds")`, `+00:00`→`Z`), and PRESERVES
`enterprise_locked`. `opt_out`/`enterprise_lock`/`enterprise_unlock`
reload-and-rewrite with ids and consented_at carried through verbatim
(empty strings when never opted in). Written file:
`json.dumps(asdict, indent=2) + "\n"`, key order `share_usage,
install_id, salt, consented_at, enterprise_locked`; write failures are
silent.

### 3.4 Output
`on`: `Sharing on. Install id: {id}` + the ADR-041 one-liner. `off`:
`Sharing off. Nothing will be sent.`. `off --enterprise` / `off
--enterprise --unlock`: the lock/unlock sentences (ADR-086). `status`:
`Sharing: {off|on|locked (enterprise)}` (lock wins over sharing) /
`Install id: {id or '(none)'}` / `Consented at: {ts or '(never)'}` /
`Consent file: {path}`, then a 5th line locked-note XOR sharing-note
(lock checked first, never both). The `no PostHog key` /
`Endpoint key: not configured` lines print ONLY when the compiled-in
`POSTHOG_API_KEY` is empty; the reference build's key is non-empty, so
they are absent from every captured run — the Rust engine embeds the
same non-empty constant (the empty-key kill switch survives as a
build-time edit, `clippy::const_is_empty` allowed on the checks).

## 4. The ADR-046 recorder (cross-cutting)

The oracle's `cli.main` appends ONE content-free event per dispatched
command — `{schema_version, ts: isoformat() µs (µs field omitted when
zero), session: token_hex(8) per process, command, outcome: ok|error,
duration_ms}`, `ensure_ascii=False` compact JSON — to
`rac-usage.jsonl`, if and only if `load_consent().share_usage` is
true at record time (so `telemetry on` records itself; `telemetry
off` does not). argparse-level exits (parse errors, `--version`,
`-h`) never record: the oracle computes the command name only after
`parse_args` returns. Mirrored in `cli::run` via a parse-level-exit
flag raised by `argparse_error()` and every version/help early
return; verified side-by-side (shape-identical events, identical
record/skip decisions across templates/validate-error/mutex-error/
--version). This channel is inherently nondeterministic (wall-clock
ts, random session, measured duration) and is NEVER byte-refereed;
telemetry cases pin their `XDG_STATE_HOME` into the sandbox so the
recorder's write stays out of the captured set. One residual nuance:
a crash inside a command records outcome `exception` in the oracle
vs `error` here (the Rust engine cannot distinguish exit-1 kinds) —
unobservable outside the unrefereed log.

## 5. Determinism boundary and referee strategy

- `telemetry on` mints install_id/salt (32 lowercase hex) and
  consented_at (UTC seconds Z) with NO oracle seam (no `DECIDED_ID_SEED`
  analogue exists in consent.py; the Python tree is frozen), so the
  harness gained `mask-consent-mint`: 32-hex runs at word boundaries →
  `<MASKED-HEX32>`, `\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z` →
  `<MASKED-UTC-TS>`, applied to stdout AND captured files. Cases that
  referee id PRESERVATION seed non-hex ids (`fixed-install-id`) and a
  non-Z-form consented_at, so a wrongly re-minted value stays visible
  through the mask.
- State seeding uses the B0 sandbox `write` steps with per-case
  `XDG_STATE_HOME`/`XDG_CONFIG_HOME` pinned to `{SANDBOX}/xdg-*`;
  raw-path-printing outputs (`Log:`, `Consent file:`, `guide.path`)
  normalize with `mask-sandbox-path`.
- `usage --share` embeds `guide.path` percent-ENCODED inside the URL,
  which `mask-sandbox-path` cannot reach — those cases instead set a
  repo-RELATIVE `XDG_STATE_HOME` (`rust/fixtures/closure/state/...`),
  identical bytes on both engines under the shared repo-root cwd, so
  the whole URL compares raw. Both engines honor relative XDG values
  verbatim (no canonicalization) by contract.

## 6. Non-UTF-8 state logs — the pinned oracle crash

`usage.read_usage` and `mcp.telemetry.read_events` wrap `read_text`
in `except OSError` ONLY, so a non-UTF-8 log raises
`UnicodeDecodeError` out of the command: traceback to stderr, EMPTY
stdout, exit 1. Mirrored bug-for-bug (`state_log_crash()`), pinned by
`usage-crash-bad-utf8-log` / `mcp-stats-crash-bad-utf8-log` over the
`rust/fixtures/closure/state/bad-utf8/` fixture (0xFF bytes; seeded as
repo files because harness `write` steps carry JSON strings, which
cannot encode invalid UTF-8). The CONSENT file is the asymmetry: its
loader catches `ValueError`, so the same bytes there mean default
no-consent, exit 0 (`telemetry-status-badutf8-consent`).

---

## 7. Parity evidence

`rust/parity-cases-closure.json`: `usage-*` (16), `mcp-stats-*` (14),
`telemetry-*` (25); state fixtures under
`rust/fixtures/closure/state/`. Proven oracle-vs-oracle over the whole
closure file (176/176) before the port — including the new
`mask-consent-mint` harness normalization — plus the existing CLI
suite oracle-vs-oracle (130/130, harness changed). After the port:
oracle-vs-rust 16/16, 14/14, 25/25; all prior closure prefixes re-run
green (diff 11, inspect 15, improve 13, portfolio 11, coverage 12,
decisions-for 13, gate 18, doctor 21); CLI 130/130; retrieve 44/44
(oracle-next); MCP 56/56 + 76/76 (output/pycompat/cli/commands were
touched); `cargo test --release` green; workspace clippy `-D warnings`
clean. Off-suite differential probes verified byte-identical: log
variants (short/absent/non-string ts, empty command/session, non-dict
JSON lines, whitespace-only and zero-byte logs, negative durations,
unicode day buckets, state-dir-as-file, log-path-as-directory),
consent variants (duplicate keys last-wins, CRLF JSON, float/negative-
zero flags, unicode ids, container values, opt-in after decline,
lock-over-sharing, unlock preserving sharing), and the recorder's
record/skip decision matrix.

Known divergences (documented, out of parity scope — pathological
inputs RAC never writes):
- Python `json.loads` accepts `NaN`/`Infinity` literals; serde_json
  rejects them. A consent file `{"share_usage": NaN}` reads as sharing
  ON in the oracle (`bool(nan)` is True) vs the tolerant default here;
  such a log LINE is an event there vs a skipped line here.
- Integers beyond i64 (~9.2e18): arbitrary-precision ints stay
  `isinstance int` in the oracle (a 10^20 `duration_ms` averages, and
  the printed average itself exceeds i64); serde parses them as f64,
  so they are excluded from the average here.
- Lone-surrogate escapes (`"\ud800"`) in a log line: accepted by
  Python (then crash the stdout encoder), rejected (skipped) here.
