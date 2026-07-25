# 17 — Closure export modes: --html, --agent-rules, --okf

Scope: the B6 modes ported for roadmap:native-cli-closure — the three
previously STUBBED `rac export` write modes. Every claim below was
verified against the oracle (`.venv-oracle/bin/rac`,
`0.1.dev50+g21c8be403`, Python 3.11.15). Source files: `src/asdecided/cli.py`
(`cmd_export`, `_cmd_agent_rules`, `_agent_rules_root`),
`src/asdecided/output/{portal,okf}.py`, `src/asdecided/services/{agent_rules,
recency}.py`, `src/rac/core/okf.py`, `src/asdecided/output/{human,json}.py`.
Rust: new `rac-engine/src/{portal,agent_rules,okf}.rs`; the vendored
shell under `rac-engine/assets/portal/`; `gitinfo.rs`
(`first_committed`), `pycompat.rs` (`py_normpath`/`py_abspath`/
`py_relpath`), `pyjson.rs` (`dumps_canonical_sorted`), `export.rs`
(`ExportArtifact.tags`), `output.rs`
(`render_agent_rules_{human,json}`), `commands.rs` (`cmd_export`
dispatch + `cmd_agent_rules`). The argv surface (mode mutex group,
`--client` choices, `--out` value handling, generic `--version`
pre-scan — export is NOT order-aware) was already in `cli.rs::run_export`
and is unchanged.

Shared dispatch order in `cmd_export`, measured: not-a-directory (exit 2)
→ agent-rules dispatch → `--check requires --agent-rules` → `--client
requires --agent-rules` → `--json cannot combine with --html or --okf` →
`--out requires --html or --okf (--json writes to stdout)` → documents →
graph → build export → okf → default JSON → html. So `export <dir>
--check --json --html` reports the `--check` error, and a missing
directory beats every mode error (pinned:
`export-html-check-precedes-json-conflict`, `export-html-err-not-a-dir`).

---

## 1. `export --html [--out FILE]`

`html = shell.replace(seam, populated)` over the packaged Portal shell
(`rac/templates/portal/lore-portal-shell.html`, vendored from lore-web @
ed4dd42, 182669 bytes). The shell is embedded via `include_str!`
(`portal::SHELL`); the unit test
`portal::tests::embedded_shell_equals_python_package_file` pins byte
identity with the Python package file plus the 182669 size — re-vendor
`rac-engine/assets/portal/` in lockstep whenever the oracle's shell
changes, or the whole ~184KB output drifts.

The seam is exact, with NO whitespace inside the element:
`<script type="application/json" id="lore-export"></script>`; the
populated form substitutes `render_export_json(export)` (the already-
ported default mode, byte-shared) after `_escape_for_script`: two literal
`str.replace` passes in this order — `</` → `<\/`, then `<!--` →
`<\u0021--` (both valid JSON escapes; the payload parses unchanged).
`PortalShellMissing` is unreachable with a compile-time embed;
`PortalSeamMissing` (count != 1) is retained as a guard → `rac: <msg>`,
exit 2.

The embedded payload carries `corpus.rac_version`, so byte parity of the
written file requires the `DECIDED_RS_VERSION` seam set to the oracle's
version (the html cases pin `0.1.dev50+g21c8be403`; no JSON-field mask
can apply to an HTML capture). Everything else is deterministic.

Write: `Path(out).write_text(html, utf-8)` — NO parent mkdir, so a
missing directory is the OSError path. `out` defaults to
`lore-export.html` relative to the CWD. Exit 0 stdout:
`wrote {out} — {N} artifact(s), {M} relationship(s)` (em-dash, `{out}`
verbatim as given). OSError → `rac: cannot write {out}: {exc}`, exit 2,
nothing written.

## 2. `export --agent-rules [--check] [--client C]... [--json] [--out ROOT]`

ADR-067. Projection: walk the corpus, keep decisions where the casefolded
first non-empty `## Status` line == `accepted` AND not in the casefolded
spec `retired_status` set (spec-driven, never hard-coded); entry =
`{identifier, title (or identifier), category}` where category is the
first non-empty `## Category` line or null; order by
`(identifier.casefold(), identifier)`.

Digest — the crux: sha256 over `json.dumps(entries, sort_keys=True,
separators=(",", ":"), ensure_ascii=False)` — keys reorder to
category,identifier,title; non-ASCII titles stay raw UTF-8; a missing
category is `null`, never omitted (`pyjson::dumps_canonical_sorted`,
pinned by `canonical_sorted_dialect` and end-to-end by the seeded-fixture
cases: `rust/fixtures/closure/export/seeded/` carries oracle-generated
targets whose digest the engine must re-derive identically or every
in-sync state flips).

Managed block (no trailing newline; the merge adds it): BEGIN marker
carrying the digest, the fixed generated-header comment, `## Settled
decisions (RAC)`, blank, the fixed prose line, blank, one
`- **<id>** — <title>[ _(<category>)_]` per entry (or `_No live decisions
recorded yet._`), END marker. `merge_managed_block`: None/blank existing
→ `block+"\n"`; existing WITH block → replace `[BEGIN..END+len]` keeping
before/after verbatim, ensure a single trailing `\n`; existing WITHOUT
block → `existing.rstrip("\n") + "\n\n" + block + "\n"`. Generate skips
the write when the embedded digest already matches (idempotent, mtime
untouched); a blank existing file reads as `[updated]`, not `[written]`.

Targets, fixed order regardless of `--client` order or duplicates:
AGENTS.md, CLAUDE.md, .github/copilot-instructions.md, .cursor/rules.
Root resolution (`_agent_rules_root`): `--out` wins (PurePosixPath-
normalized `str()`); else `Path(directory.rstrip("/"))` — a `rac`-named
final component yields its parent (parent `.`/empty → `.`), else the
directory itself. `rac export rac --agent-rules` therefore writes into
the CWD and prints `Output root:   .`.

Human output: bold title `Agent Rules` / `Agent Rules — drift check`, a
`=` underline of the title's code-point length, `Corpus digest: <hex>`,
`Output root:   <root>` (three spaces), blank, per-file
`  <icon> <path>  [<state>]` (`+` written, `~` updated, green `✓`
in-sync, red `✗` stale/missing — colour TTY-gated, plain under the
harness), blank, verdict (`✓ Wrote/updated N file(s).` / `✓ All targets
already in sync — nothing to write.` / `✓ In sync — every present target
matches the corpus.` / `✗ Drift — N file(s) stale or missing the block.`
+ `  Regenerate: rac export --agent-rules`). `--json`:
`json.dumps(indent=2)` with ensure_ascii DEFAULT (True — unlike the
coverage JSON): `{mode, digest, root, files:[{client,path,state}]}`.

Exits: generate ok / check clean → 0; check drift (any stale or missing)
→ 1; OSError during generate → `rac: cannot write under {root}: {exc}`,
exit 2. Invalid `--client` values are argparse-choice errors in the
shared parser, so `unknown_clients` is unreachable and not ported.

## 3. `export --okf [--out DIR]`

ADR-048. Bundle = one Markdown file per exported artifact at
`os.path.relpath(art.path, directory)` (`pycompat::py_relpath` —
lexical `abspath` on both sides against the CWD, then the component
walk), plus generated `index.md` and `log.md`; written in
`sorted(bundle.items())` order (BTreeMap; byte order == code-point
order), per-file `mkdir -p` + write. `out` defaults to `okf-bundle`.
Exit 0 stdout: `wrote {out}/ — {N} artifact(s), {M} relationship(s)`
(note the slash). OSError mid-loop → `rac: cannot write {out}: {exc}`,
exit 2 (files already written stay written; the same sorted order keeps
partial trees engine-identical).

Artifact file: `---` / `type: <OKF_TYPE[t]>` (requirement→Requirement,
decision→ADR, design→Design, roadmap→Roadmap, prompt→Prompt) / `id:` /
optional `created:` / optional `updated:` / optional
`tags: [a, b]` (comma-space join, present-only) / `---` / blank / body
(`split_frontmatter(text).body.strip()`, text-mode re-read) / optional
blank + `# Citations` + blank + `- [<target.title>](<target relpath>)`
per resolved outgoing edge (edge.from == art.id AND edge.to in the
corpus id map, dict-last-wins; unresolved edges omitted; relationship
order). Every file ends with exactly one `\n`.

`created`/`updated` derive from git with the committer's STORED offset
(`%cI`, TZ env ignored): `updated` = `git log -1 --format=%cI`,
`created` = first non-blank line of `git log --reverse --format=%cI`
(new `gitinfo::first_committed`), both `fromisoformat().isoformat()`
round-tripped and gated by parseability (`_parse_stamp` → None). Outside
a repo / untracked / uncommitted → fields omitted; `log.md` groups by
the stamp's civil-date prefix (`committed.date().isoformat()` — no tz
conversion), days newest-first, within a day path-sorted; no history at
all → `# Log\n\n_No commit history available._\n`. Pinned by
`export-okf-git-pinned` (three commits across +02:00/-05:00/+09:00
offsets, a same-day pair, and an uncommitted artifact) via the harness
scripted-git fixture.

`index.md`: `# <corpus_name> — Knowledge Index`, the
`A derived OKF bundle of N artifact(s)…` overview (singular noun at
N==1), then non-empty type sections in the fixed order Requirements,
Decisions, Designs, Roadmaps, Prompts, artifact (sorted-path) order
within.

LANDMINE, pinned: an artifact whose bundle key is `index.md`/`log.md`
raises an UNCAUGHT ValueError in the oracle — a Python traceback on
stderr, exit 1, nothing written (normally prevented by the
okf-reserved-filename validate gate). The port mirrors exit 1 +
no-write with a one-line `ValueError: …` stderr
(`export-okf-err-reserved-collision-writes-nothing`).

`ExportArtifact` gains `tags` (from frontmatter metadata) for this
projection only — like the oracle's dataclass field, it is deliberately
NOT in the viewer JSON `to_dict` (ADR-007 unchanged).

## 4. Divergences (documented, stderr never byte-refereed)

- OSError message tails: the oracle embeds Python's
  `[Errno N] <strerror>: '<path>'` (with the INTERNAL failing subpath,
  e.g. `cannot write /o: [Errno 20] Not a directory: '/o/decisions'`);
  Rust emits `io::Error` Display text. Prefix `rac: cannot write …` and
  exit 2 match; stderr bytes differ by design.
- The reserved-filename collision prints a full Python traceback in the
  oracle vs one `ValueError:` line here; exit 1 and the empty written
  tree match.
- Oracle crash surfaces (strict-utf8 re-reads of artifact bodies or
  agent-rules target files, UnicodeDecodeError) degrade instead of
  crashing, per PORT-CONTRACT decision 3 — closure fixtures stay
  healthy so no case exercises them.

## 5. Case inventory

43 cases in `parity-cases-closure.json`, proven oracle-vs-oracle before
the port: `export-html-*` (11 — written-file byte captures with the
pinned version seam, default/explicit/`--out=` paths, empty corpus,
subcorpus naming, escape-bearing bodies via the fixture, mutex/conflict/
not-a-dir/write-failure refusals with no-write proofs, `--version`
ordering), `export-rules-*` (20 — fresh generate, seeded idempotence,
clean/stale/missing checks incl. a `--client` subset, splice/append/
preserve/blank-file merges, root resolution `.`/trailing-slash/`--out`,
JSON generate + JSON drift, client order/duplicates, empty corpus,
write-failure refusal), `export-okf-*` (12 — the pinned-git tree,
non-git degrade, default out, `.` directory, single-artifact noun,
empty corpus, subcorpus root, conflict/not-a-dir/write-failure/reserved-
collision refusals with no-write proofs, `--version` ordering). Fixtures:
`rust/fixtures/closure/export/{proj,seeded}` (seeded = proj + the four
oracle-generated agent-rules targets, digest
`7bfb072f0f6fa31a858c241f7cc578e14855b54db255f50feb8cc8cc8daeb790`).
