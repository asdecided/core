# PARITY-REPORT — Rust engine spike (roadmap:native-engine-spike)

Final report. Status of the byte-parity claim for the experimental Rust port of the
rac-core engine against the frozen Python oracle (`src/` at commit
`21c8be4`, installed as `.venv-oracle`, version `0.1.dev50+g21c8be403`).
Everything here is machine-checked: the referee is `rust/parity-harness`,
whose own correctness was adversarially verified (it detects single-byte
stdout corruption, exit-code corruption, and corruption on normalized
cases) before any engine claim was accepted.

## Headline

**130/130 parity cases pass byte-for-byte** (the original 118 plus 12 regression cases pinned by the fuzz campaigns) — identical stdout bytes and
identical exit codes — across the covered command set, on the live `rac/`
corpus (417 artifacts) and the fixture corpora, in human, `--json`, and
`--sarif` output modes. Verified twice per run with byte-identical
scoreboards, from forced clean rebuilds, by an independent verifier agent,
plus dozens of fresh adversarial probes outside the case list.

## Covered command set

`validate` (file / dir / stdin / `--corpus`, human+json+sarif),
`relationships` (inspection + `--validate`, human+json+sarif), `find`,
`resolve`, `review` (human+json+sarif), `stats`, `schema` (+ `--template`),
`export` (default/`--json`/`--graph`/`--documents` JSONL), `--version`
(root and per-subcommand), and the error/exit-code paths of each.

## Fuzzing (divergence hunt)

Campaign 1 (validate/relationships/stats matrix): 4,866 distinct generated
inputs, ~72,000 engine-pair command executions, 32 mutation operators.
Outcome:

- **1 real Rust bug** (`003-rust-bug-bigint-i64-seam`): integers beyond
  i64 aborted the frontmatter load instead of following Python bignum
  semantics. Status: FIXED — arbitrary-precision integers across all
  PyYAML bases, exact bignum/float equality in duplicate-key detection,
  full-value message printing, CPython's 4300-digit conversion-limit
  crash mirrored as a documented marker; pinned as vectors and verified
  independently.
- **1 port-consistency finding** (`002`): one oracle-crash constructor
  path emitted a regular issue instead of the documented divergence
  marker. Status: FIXED — every oracle-crash constructor path now emits
  the same marker (map-on-scalar, timestamp edge cases audited).
- **1 intentional divergence class** (`001-oracle-crash-unhashable-key`):
  unhashable YAML keys, constructor/tag mismatches (`!!int ''`), and
  out-of-range timestamps crash the Python oracle uncaught (traceback,
  empty stdout). The Rust engine deliberately reports a marked
  `internal-oracle-divergence` issue instead of mirroring the crash
  (PORT-CONTRACT decision 3). Repros preserved under
  `rust/fuzz/pinned/oracle-crashes/`.
- Oracle nondeterminism: none observed.

Campaign 2 (full command matrix: adds resolve, find, schema, export,
review, relationships inspection, stdin validation, DECIDED_MAX_FILE_BYTES
variation, path edge forms, multi-file corpora; 10 engine-pair runs per
input): ~4,800 further inputs across seeds 201-306. Found and FIXED eight
more engine bugs — surrogate handling on stdin, DECIDED_MAX_FILE_BYTES
Python-int() env parsing, CRLF export edges, fence-at-EOF rendering, a
stats largest-file tie-break, export --documents/--json body_html edge
cases, and C0 control stripping in tight list items (markdown-it-py
strips inline content with Python str.strip(), whose whitespace set
includes U+001C-001F; the Rust strip rule initially missed list items
because the markdown-it crate splices tight paragraphs into ListItem
nodes — closed with a 591-case oracle-generated C0 grid). Each fix is
pinned as a parity case or vector suite. The campaign closed on a strict
consecutive dry pair (seeds 305, 306: 800 inputs each, zero new
signatures of any kind) with a third fully-dry round (301) as
corroboration; full evidence in `rust/fuzz/CAMPAIGN-2.md`. Total across
both campaigns: ~13,000 distinct inputs, ~120,000 engine-pair command
executions, 9 engine bugs found and fixed, zero unexplained divergences
remaining.

## Oracle re-pin and the retrieval surface

The oracle was re-pinned to the latest `origin/main` after the spike's
main phases closed: `src/` is byte-identical between the original pin
(`21c8be4`) and current main, so the 130/130 claim carries over unchanged
(re-verified by a full run after the merge). The in-flight
grounding-retrieval surface (`rac retrieve` + the `retrieve_grounding`
MCP tool, roadmap:grounding-retrieval-surface, unmerged branch at
`f2091be`) was ported ahead of its merge against a second oracle pinned
to that branch head: **44/44 retrieve parity cases byte-identical**
(`rust/parity-cases-retrieve.json`, run twice), mainline 130/130 intact,
independently verified with fresh adversarial probes (supersedes forks
and cycles, zero-overlap scope binding, budget=1 truncation, empty
corpus). Recon confirmed the branch's service deltas are purely additive
to existing commands; its sole existing-surface byte change (the root
argparse choices list gains `retrieve`) is deliberately not adopted
until the branch merges. With this, every command the six-tool Lore MCP
surface depends on has a parity-proven Rust implementation.

## asdecided-spec acceptance suite

The public specification repo (itsthelore/asdecided-spec, v0.1.0) ships an
executable acceptance suite (`examples/manifest.json`: 2 valid corpora +
16 invalid cases with expected blocking finding codes). Both engines were
run over all 18 cases with identical argv/env: **18/18 byte-identical
stdout and exit codes**, every exit matching the manifest's expectation.
This is the first second implementation of the spec, and it makes the
spec's implementation-neutrality claim ("any conformant implementation
must agree") a demonstrated property rather than an intention. At
mainline, the Rust engine should be certified against asdecided-spec's
`schema/` and `vocabulary/` directly, retiring this spike's derived
`rust/spec/artifact-specs.json` extraction.

## Known, documented divergences

1. **Oracle crash inputs (001 class)** — divergence by design, see above.
   These inputs are unreachable from any valid corpus; the maintainer
   should decide at mainline time whether a native engine should mirror a
   crash or fix it (we recommend: report, don't crash — and patch the
   Python oracle separately).
2. **Version strings** — the oracle emits a setuptools-scm git-describe
   version. The Rust binary takes `DECIDED_RS_VERSION` (spike seam); the
   harness pins it to the oracle's exact string, so comparisons stay
   byte-for-byte. A mainline port would build the same version string in.
3. **argparse usage/help bodies** — width-wrapped by Python's
   HelpFormatter; out of parity scope by contract decision 9. The final
   `<prog>: error: <msg>` line, stderr routing, empty stdout, and exit
   code 2 are matched.

## Normalizations (declared, case-scoped)

Only where output is environment-derived, mirroring the golden tests'
conventions: `strip-recency-json` / `strip-stale-human` (git-derived
recency in `find`), `mask-version` (build-derived version in
`export --json` / SARIF `driver.version`). 16 of 130 cases; all other
cases compare raw bytes. The verifier audited every normalization against
the contract and confirmed none hides real bytes.

## Gap list (not covered by the parity suite)

- Commands out of spike scope by the roadmap artifact: explorer TUI,
  `ingest`, MCP serving (`mcp`, `mcp-stats`), and the derived-index
  cache/store (the Rust engine has no cache by design; the oracle's cache
  is contractually byte-neutral and was verified so).
- Non-covered commands with no parity cases: `diff`, `inspect`, `improve`,
  `rename`, `doctor`, `coverage`, `gate`, `watchkeeper`, `portfolio`
  (ported only as far as review needs it), `index`, `telemetry`, `usage`,
  `new`, `templates`, `init`, `quickstart`, `decisions-for`, `eval`,
  `migrate`, `skill`, `hook`.
- `export --html` / `--okf` / `--agent-rules` success paths are covered
  only where parity cases exercise them; file-writing side effects are
  compared on the `wrote ...` stdout lines and exit codes.
- Exit-2 usage-error cases compare stdout (empty) and exit code, not the
  argparse-wrapped stderr body (contract decision 9).
- TTY-gated ANSI color: asserted by code inspection and spot check, not
  by the piped-stdio scoreboard.
- Unreadable-file (EACCES) paths: untestable as root in this container;
  both engines agreed on the substituted probes (dangling symlink,
  directory-named-`.md`).

## The MCP serving surface — now prototyped natively

Following the engine-side closure, the serving layer itself was ported:
`rust/rac-mcp` is a native stdio MCP server implementing the full six-tool
Lore surface (including `retrieve_grounding`) over the parity-proven engine
services, per the wire contract in `PORT-CONTRACT.d/10-mcp-surface.md`
(framing, handshake, pinned schema bytes, the ADR-033 budget mechanism
ported bug-for-bug, ADR-034 error shapes, ADR-032 stateless reads).

Wire parity: **56/56 frames byte-identical vs the mainline Python server
and 76/76 vs the retrieval-branch server** (run twice each, byte-identical
scoreboards, independently verified with harness break-tests and 20+ fresh
adversarial probes). Serving latency on the live corpus (7-run medians):
startup-to-first-tool-result 2 206 ms (Python) vs 35 ms; `get_summary`
1 973 ms vs 39 ms; `retrieve_grounding` 1 117 ms vs 172 ms.

**Oracle defect found by this verification** (documented in contract
section 0a): Python's cache-on path computes deduped document frequency
while its no-cache path double-counts duplicate query tokens — warm and
cold Python return different ranking-evidence bytes for duplicate-token
queries, violating ADR-112's byte-identical invariant. The Rust engine
matches the no-cache path (per contract 06); the MCP harness pins
`DECIDED_NO_CACHE=1` on both servers as the canonical comparison path. This
needs an upstream Python fix at mainline, after which the pin can drop.

Declared gaps: HTTP transport (ADR-098) not ported (stdio only);
audit/telemetry sidecars stubbed behind a documented seam (contractually
wire-neutral, verified); `resources/list` inferred, excluded from the
certified basket.

## Superseded: the original path options

The pre-port analysis below is retained for the decision record.

The MCP server is the most consequential unported surface, but the recorded
architecture bounds the problem tightly: ADR-030 makes the server
tools-only, ADR-031 forbids it from owning intelligence (it imports
`rac.services` in-process and only shapes results), and ADR-032 makes every
tool call a stateless full re-read of the repository. There is no stateful,
intelligent server to port — the intelligence below it is exactly what this
spike ported and parity-proved.

The spike's numbers strengthen the recorded posture rather than straining
it: ADR-032 accepted stateless reads while a full corpus read cost
milliseconds and named `collect_corpus` as the optimization seam if scale
demanded one. The Rust engine's cache-free full read of the live corpus is
28 ms — cheaper than the Python engine's warm cache hit — so the stateless
contract holds permanently and the freshness machinery ADR-105 builds for
serving is not needed on a native path.

Three options work within the recorded decisions, in ascending effort:

1. **Status quo during transition** — the Python server stays on the Python
   engine; the conformance suite keeps both engines byte-agreeing. Dual
   maintenance; acceptable only as a temporary state.
2. **Bindings** — the Python server consumes the Rust core in-process via
   native bindings (e.g. PyO3). ADR-031's in-process rule holds literally;
   one source of truth; server code untouched. The binding layer becomes new
   contract surface.
3. **Native server** — port the thin server layer itself (protocol plumbing
   plus presentation shaping, per ADR-031's own boundary), taking ADR-098's
   shared HTTP serving with it. Cleanest single-engine end state.

A fourth path — the server shelling out to the 2.2 ms Rust CLI per tool
call — is newly viable on the numbers but was explicitly considered and
rejected by ADR-031 when the CLI cost ~200 ms. Adopting it would mean
re-opening that accepted decision with this new evidence, not working
around it; it is recorded here as evidence for that file, not as a
recommendation.

## Reproduce

```sh
cd rust && cargo build --release
target/release/parity-harness \
  --engine-a ../.venv-oracle/bin/rac --engine-b target/release/rac \
  --cases parity-cases.json --scoreboard-dir parity-out
python3 fuzz/difffuzz.py --seed 99 --rounds 4 --batch 100
```
