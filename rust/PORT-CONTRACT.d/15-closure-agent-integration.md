# 15 — Closure agent integration: skill, hook, eval

Scope: the B4 commands ported for roadmap:native-cli-closure —
`rac skill`, `rac hook` (agent-integration installers, both WRITE), and
`rac eval` (the ADR-066 grounding benchmark and CI gate). Every claim
below was verified against the oracle (`.venv-oracle/bin/rac`,
`0.1.dev50+g21c8be403`, Python 3.11.15). Source files: `src/asdecided/cli.py`
(`cmd_skill`/`cmd_hook`/`cmd_eval`), `src/rac/core/{skills,hooks}.py`,
`src/asdecided/services/{skill,hook,eval}.py`, `src/asdecided/skills/*/SKILL.md`,
`src/asdecided/hooks/*.sh`, `src/asdecided/output/{human,json}.py`
(`render_skill_*`, `render_hook_*`). Rust: new `rac-engine/src/skill.rs`,
`hook.rs`, `eval.rs`, `sha256.rs`; vendored assets under
`rac-engine/assets/{skills,hooks}/`; `output.rs` (`render_skill_*`,
`render_hook_*`), `commands.rs` (`cmd_skill`/`cmd_hook`/`cmd_eval`),
`cli.rs` (`run_skill`/`run_hook`/`run_eval`, order-aware pre-scan
exemptions), `walk.rs` (`py_join`), `pycompat.rs` (`py_format_fixed`).

---

## 1. `rac skill <action> [name] [--dir DIR] [--json]`

### 1.1 Registry
Five bundled skills, fixed registry order — `rac-artifacts`,
`rac-review`, `rac-ingest`, `rac-import`, `rac-capture` — each a single
packaged `SKILL.md` (3428 / 3700 / 4046 / 6546 / 8021 bytes). The Rust
port embeds byte-identical vendored copies
(`rac-engine/assets/skills/<name>/SKILL.md`, `include_bytes!`); the
unit test `skill::tests::embedded_bytes_equal_python_package_files`
pins the identity against the Python package files, because the
INSTALLED file must be byte-identical to what the oracle installs.

### 1.2 Argv surface (order-aware)
`action` is a required positional with choices `{install,list}`
validated when the token is CONSUMED: `skill bogus --version` exits 2
(argparse invalid-choice), `skill --version bogus` prints the version.
`name` is an optional positional; a THIRD positional defers to the
root parser's end-of-parse `unrecognized arguments` (exit 2, and —
pinned by case `skill-err-extra-positional-nothing-written` — nothing
is written, since parse fails before the handler). `--dir` default
`"."`. `skill` and `hook` are therefore in `cli.rs`'s order-aware set,
exempt from the generic `--version` pre-scan.

### 1.3 `list`
`list` with a name → `rac: skill list takes no skill name`, exit 2.
Human: bold-gated `Bundled agent skills:`, blank, `- <name ljust w>
<desc>` rows (two spaces between columns), `w = max(len(name))` = 13,
registry order. JSON (`indent=2`, ensure_ascii=True):
`{"schema_version":"1","skills":[{"skill":…,"description":…},…]}`.

### 1.4 `install` — the two-phase, all-or-nothing write
Check order is load-bearing:
1. `Path(dir).is_dir()` → else `rac: not a directory: <dir>` exit 2 —
   BEFORE name validation (`skill install bogus --dir /nonexistent` is
   a not-a-directory error, pinned by
   `skill-install-err-dir-check-precedes-name`);
2. unknown name → `rac: unknown skill: <name> (available: rac-artifacts,
   rac-review, rac-ingest, rac-import, rac-capture)` exit 2;
3. EVERY destination `<dir>/.claude/skills/<name>/SKILL.md` is checked
   before ANY write. One collision refuses the whole install, exit 1:
   single — `rac: <path> already exists; rac skill install never
   overwrites`; multiple — `rac: <n> skill files already exist; rac
   skill install never overwrites:` + `  - <path>` lines in REGISTRY
   order filtered to the existing ones. Nothing is written and existing
   files keep their bytes (captured-tree cases).
4. write phase: `mkdir -p` parents, `write_bytes` — no chmod.

Emitted paths are `str(Path(dir) / ".claude" / "skills" / name /
"SKILL.md")` — pathlib-normalized, NEVER abspath'd: `--dir .` yields
`.claude/skills/…` (the bare `.` vanishes), a relative dir stays
relative, an absolute dir stays absolute (`walk::py_join`).

Human: `Installed <name> skill: <path>` per skill, blank, `Claude Code
discovers skills automatically from .claude/skills/ in the project.`
JSON: `{"schema_version":"1","installed":true,"skills":[{"skill":…,
"path":…},…]}` — `bytes_written` is in the oracle's dataclass but NOT
in `to_dict`, so the Rust model does not carry it.
`SkillResourceMissing` (broken Python installation, exit 1) has no
Rust equivalent: embedded resources cannot be absent.

---

## 2. `rac hook <action> [--style {post-commit,pre-commit}] [--dir DIR] [--json]`

### 2.1 Registry
Two bundled hooks, registry order `post-commit` (default, first) then
`pre-commit`; packaged as `<style>.sh` (572 / 625 bytes) but INSTALLED
as `<dir>/.git/hooks/<style>` with NO extension. Vendored byte-identical
copies under `rac-engine/assets/hooks/`, pinned by
`hook::tests::embedded_bytes_equal_python_package_files`.

### 2.2 Argv surface (order-aware)
`action` choices validated at consume (like skill). `--style` is
argparse-choice-validated when its VALUE is consumed: `--style bogus`
exits 2 with the argparse invalid-choice error at that position (so it
beats a later `--version`, and the service-level `HookNotFound` is
unreachable via the CLI); an earlier `--version` wins. `list` ignores
`--style`/`--dir` entirely.

### 2.3 `list`
Human: bold-gated `Bundled git hooks:`, blank, `- <style ljust 11>
<desc>` rows. JSON: `{"schema_version":"1","hooks":[{"style":…,
"description":…},…]}`.

### 2.4 `install`
Check order: `Path(dir).is_dir()` → `rac: not a directory: <dir>`
exit 2; then `<dir>/.git` must be a DIRECTORY — a `.git` FILE (worktree
/ submodule pointer) fails the same way: `rac: no .git directory in
<dir>; run \`rac hook install\` from a git repository root`, exit 2 (no
git subprocess — a bare mkdir'd `.git` satisfies it); existing
destination → `rac: <path> already exists; rac hook install never
overwrites`, exit 1, file untouched (bytes AND mode — pinned with
`compare_file_mode`). Then `mkdir -p <dir>/.git/hooks`, write bytes,
and chmod `st_mode | S_IXUSR | S_IXGRP | S_IXOTH` — the executable bit
is part of the contract (git will not run a non-exec hook); observed
0755 under umask 022. Styles coexist: installing `pre-commit` next to
an existing `post-commit` succeeds.

Human: `Installed <style> git hook: <path>` + blank + `Git runs it
automatically on each commit. Remove the file to stop it.` JSON:
`{"schema_version":"1","installed":true,"hook":{"style":…,"path":…}}`.
Paths are pathlib-joined like skill's (never abspath'd).

### 2.5 Harness note
The parity harness's post-run capture excludes `.git` trees UNLESS a
capture pattern explicitly names a `.git` component; hook cases capture
`…/.git/hooks/*` so only the written hook files are refereed, never
git internals.

---

## 3. `rac eval [--check | --update-baseline] [--json] [--root ROOT] [--queries QUERIES] [--baseline BASELINE] [--config CONFIG]`

### 3.1 Argv surface (order-aware)
NO positionals — any bare token defers to the root `unrecognized
arguments` (exit 2). `--check|--update-baseline` is a mutually
exclusive group erroring at the ENCOUNTER of the conflicting flag
(direction-sensitive message; beats a later `--version`). Defaults
resolve against the CWD: `tests/eval/{corpus,queries.json,
baseline.json,eval-config.json}`.

### 3.2 Determinism (ADR-066)
The scored path is a pure function of (corpus bytes, query set,
retrieval code): no embeddings, no LLM judge, no network, no clock.
Exactly TWO nondeterministic fields exist, both in diagnostic
`metadata`: `generated_at` (`datetime.now(UTC).isoformat()`) and
`lore_version` (git-describe / `DECIDED_RS_VERSION` seam). The parity
harness masks precisely those two (`mask-json-field:metadata.
generated_at`, `…lore_version`); `corpus_hash`, `query_set_hash`,
`n_queries`, all metrics and all `per_query` rows compare RAW.

### 3.3 Scored surfaces (REQ-002/004)
A `search_artifacts` case consumes `resolve::search_index(entries,
query, artifact_type=case.type)` match order verbatim over the
recursively built repository index. A `get_related` case re-walks the
corpus, resolves `case.query` via `resolve_in_index` (an unresolved
query is a usage error: `rac eval: get_related case '<id>': query
'<q>' did not resolve to an artifact in '<root>'`, exit 2), and
consumes the `incoming` neighborhood id order — sort key
`(relationship-section rank, id, source path)`, cap 1000 — mirrored in
`eval.rs` from `rac-mcp::graph::incoming_references` (rac-engine
cannot depend on rac-mcp; eval needs only the ordered id list). Eval
parity is therefore DOWNSTREAM of the search/related ports: the
`eval-report-json` case doubles as a whole-surface regression check.

### 3.4 Scoring and aggregation
P@k = |Rel ∩ top_k| / k (empty slots count against precision), R@k =
|Rel ∩ top_k| / |relevant tuple| (duplicates in `relevant` inflate the
denominator), k ∈ {1,3,5}; a violation is a `must_not_return` id in
the top-5 window, reported sorted. Results sort by case id; means are
macro (equal weight per case), rounded `round(x, 6)` (banker's — the
Rust side uses `py_round`) and serialized via `pyjson` so `1.0` /
`0.25` / `0.416667` keep Python float repr. `metrics` = `overall`
(p_at_1,3,5 then r_at_1,3,5 then `negative_violations` int) +
`by_category` + `by_tool` (sorted group names, each `{p_at_1,
r_at_5}`). `metadata` key order: `lore_version`, `corpus_hash`,
`query_set_hash`, `n_queries`, `generated_at`. `corpus_hash` =
sha256 over each walked `*.md`'s rel-POSIX path + NUL + bytes + NUL in
corpus-walk sorted order; `query_set_hash` = sha256 of the raw query
file bytes (both `sha256:<hex>`; `rac-engine/src/sha256.rs` is a
hand-rolled FIPS 180-4 with pinned vectors — the workspace stays
dependency-free).

### 3.5 Modes and exit codes
Mode wins over `--json` (`--check --json` prints the gate line only).
- default: human report to stdout, exit 0. Layout: `Overall`, header
  `{'P@k':>8}{'R@k':>8}` pairs, one row of `{v:>8.3f}` cells ordered
  p1 r1 p3 r3 p5 r5, `  negative_violations: <n>`; `By category` /
  `By tool` blocks (`width = max(len(name))`, header `  {'':w}    P@1
  R@5`, rows `  {name:<w}  {p:>6.3f}  {r:>6.3f}`); `Violations` then
  `  none` or offender lines `  <id> (<tool>): returned <violations>
  in top-5 [returned=<returned>]` where both lists render as Python
  `repr(list[str])` — single quotes, `', '` separator.
- `--json`: `{"metrics":…,"metadata":…,"per_query":…}`,
  `json.dumps(indent=2, ensure_ascii=False)`. Per-query key order:
  id, tool, category, returned, relevant, [must_not_return only when
  non-empty], p_at_1/3/5, r_at_1/3/5, violations.
- `--check`: loads baseline + config AFTER the benchmark runs (a
  missing baseline still costs a full run, then exits 2). Gate PASS →
  stdout `rac eval: gate PASS`, exit 0. Failures → one line per fired
  rule to STDOUT (byte-refereed), exit 1.
- `--update-baseline`: writes `render_metrics_json(metrics) + "\n"`
  (ends `}\n`) to `--baseline`, prints `rac eval: baseline updated ->
  <path>`, exit 0.
- Any `EvalUsageError` → stderr `rac eval: <msg>`, exit 2: corpus not
  a dir, baseline/queries/config not found, malformed shapes
  (`expected a non-empty 'cases' list` / `expected a metrics object` /
  `expected 'floors' and 'tolerance'`, per-case field checks,
  duplicate case id), and the unresolved get_related case.

### 3.6 Gate semantics (landmines)
The gate enforces ONLY `p_at_1` and `r_at_5`, and only where a floor
is declared in config — a floor on `p_at_3`/`p_at_5` is SILENTLY
ignored (unit-pinned in `eval::tests`). Rule order: (a)
`negative_violations > floors.negative_violations` (default 0) → `FAIL
[negative_violations] overall.negative_violations: limit {t:.0f},
current {c:.0f}`; then per gated pair — `overall` pairs first, then
`by_category` in sorted order — a metric ABSENT from the current run
fires RULE_FLOOR with current 0.0 (`by_category.<gone>.p_at_1`), a
present metric fires floor (`floor {t:.6f}`) and/or regression
(`baseline {t:.6f}`) when `value < baseline − tolerance`. The
regression rule also runs only over floored pairs.

### 3.7 Known divergences (stderr-only, never byte-refereed)
- Invalid-JSON inputs embed the JSON parser's message after
  `malformed <what>: <path>: ` — CPython's `Expecting value: line 1
  column 1 (char 0)` vs serde_json's `expected value at line 1 column
  1`. Exit code and prefix match; the tail differs. Same class:
  Python-repr of non-string case ids in per-case error messages, and
  `float()` accepting numeric strings in gate configs where the Rust
  side reads JSON numbers only.
- `--update-baseline` to an unwritable path: the oracle tracebacks
  (exit 1); the Rust engine prints `rac: cannot write <path>: <err>`
  and exits 1 — same code, different stderr.

---

## 4. Parity coverage

`rust/parity-cases-closure.json`: 17 `skill-` cases (list human/json,
list-with-name usage error, single/all installs human+json, default
and absolute `--dir` path shapes, all-or-nothing refusal with seeded
collisions, never-overwrite, dir-check-precedes-name ordering, missing
`--dir` value, invalid action, action/version ordering both ways,
extra-positional-writes-nothing, missing action); 16 `hook-` cases
(list human/json + flag-ignoring list, default style/dir installs,
pre-commit json, exists-untouched, styles coexist, non-git dir, `.git`
file pointer, bad style writes nothing, not-a-directory, bad action,
style/version ordering both ways, missing action — install cases
referee written bytes AND the executable bit via `compare_file_mode`);
23 `eval-` cases (report human/json on the committed benchmark and on
a violation-triggering query set, gate pass, mode-beats-json, three
forced-failure fixtures under `rust/fixtures/closure/eval/`,
update-baseline written-file bytes, relative `--root` resolution, and
the full usage-error surface). All are green oracle-vs-oracle and
oracle-vs-rust.
