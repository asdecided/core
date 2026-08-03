# 16 — Closure scaffold writes: new, init, quickstart, rename, migrate

Scope: the B5 commands ported for roadmap:native-cli-closure — `rac new`,
`rac init`, `rac quickstart`, `rac migrate metadata` (all WRITE), and
`rac rename` (writes under `--apply`). Every claim below was verified
against the oracle (`.venv-oracle/bin/rac`, `0.1.dev50+g21c8be403`,
Python 3.11.15). Source files: `src/asdecided/cli.py` (`cmd_new`/`cmd_init`/
`cmd_quickstart`/`cmd_rename`/`cmd_migrate`, `_maybe_ask_usage_sharing`),
`src/rac/core/{idgen,templates}.py`, `src/asdecided/services/{create,init,
profiles,quickstart,rename,migrate}.py`, `src/asdecided/output/{human,json}.py`.
Rust: new `rac-engine/src/scaffold.rs` (idgen, embedded templates, config
identity, init/profiles, create, quickstart, migrate) and `rename.rs`;
vendored assets under `rac-engine/assets/templates/`; `output.rs`
(`render_{new,init,quickstart,migrate,rename,rename_result}_{human,json}`),
`commands.rs` (`cmd_new`/`cmd_init`/`cmd_quickstart`/`cmd_rename`/
`cmd_migrate`, `maybe_ask_usage_sharing`), `cli.rs` (`run_*`, order-aware
pre-scan exemptions for init/quickstart/migrate), `consent.rs`
(`consent_recorded`, `decline`).

---

## 1. Minted ids and the mask referee

`generate_id` (ADR-026): `<KEY>-` + 12 Crockford-base32 chars — an 8-char
segment from `int(time.time()*1000) & (2^40-1)` plus a 4-char segment
from `secrets.randbits(20)`; alphabet `0123456789ABCDEFGHJKMNPQRSTVWXYZ`.
The oracle exposes NO seam to pin the id, so the harness `mask-ids`
normalization is the referee — applied to stdout AND captured file bytes,
because a minted id lands in both (`new`/`quickstart`/`migrate`). The Rust
generator uses the real clock and `/dev/urandom`. Collision handling:
`new`/`quickstart` check one candidate against the repository index
(canonical identifier of every walked artifact, uppercased), `migrate`
additionally dedupes within-run; both retry at most 5 times, then
`IdGenerationExhausted` — `rac: could not generate a unique artifact ID
in 5 attempts`, exit 1.

The five template bodies are embedded verbatim
(`rac-engine/assets/templates/*.md`, `include_str!`); the unit test
`scaffold::tests::embedded_templates_equal_python_package_files` pins
byte identity with the Python package files, because the written artifact
is `render_frontmatter(id, type) + body` byte-for-byte
(`---\nschema_version: 1\nid: <id>\ntype: <type>\n---\n` + verbatim body,
no trailing-newline munging).

## 2. `rac new <type> <output_path> [--json]`

Two required positionals; no directory arg — the repository identity is
discovered by walking UP from the output path's parent (resolved) to the
nearest `.decided/config.yaml`. NOT order-aware: `--version` anywhere wins
(`new bogus x.md --version` prints the version), the generic pre-scan
applies.

Check order (all before any write): `load_template(type)` → exists →
parent is_dir → config discovery → id assignment → one `write_bytes`.
Exit 2 (`rac: <msg>` stderr): unsupported type (`unsupported artifact
type: bogus (supported: requirement, decision, roadmap, prompt,
design)`), `<path> already exists; rac new never overwrites` (a DIRECTORY
at the path also "exists"), `directory does not exist: <parent>`, `no
repository identity found at or above <parent>; run \`rac init\` to
establish a repository key first`, and argparse errors (`the following
arguments are required: type, output_path` lists all still-missing).
Exit 1: malformed config / id exhaustion (operational).

Path shaping is asymmetric and load-bearing: the SUCCESS output echoes
the argv string VERBATIM (`./rac/x.md`, `rac//decisions//y.md` appear
literally in stdout and JSON), while the error messages use the
pathlib-NORMALIZED parent (`rac//nonexistent//x.md` → `directory does not
exist: rac/nonexistent`). Human: `Created <type> artifact: <path>` /
`ID: <id>` / blank / `Edit the TODO placeholders, then check it with: rac
validate <path>`. JSON (`indent=2`): `{"schema_version":"1","created":
true,"type":…,"path":…,"id":…}` — NO `bytes_written` (in the oracle's
dataclass, not its `to_dict`).

The id-collision walk covers the WHOLE repository root (the config dir's
grandparent) — the oracle-crash surface of §7.1.

## 3. `rac init [directory] [--key KEY] [--ticketing PROVIDER] [--profile NAME] [--json]`

`directory` optional (default `.`). ORDER-AWARE: `--ticketing` (choices
jira, github, linear, azure-devops, servicenow, none) and `--profile`
(default, enterprise) are argparse-choice-validated when the VALUE is
consumed — `init --profile bogus --version` exits 2, `init --version
--profile bogus` prints the version — and a missing `--key` value errors
at its own position (`--key --version` → `expected one argument`, exit 2).

Semantics: fresh init writes `<dir>/.decided/config.yaml` as hand-built
string concatenation (NOT a YAML dump): `repository_key: <KEY>\n`, then
optional `ticketing:\n  provider: <p>\n`, then the profile stanza
verbatim (enterprise: a fixed comment + `enforcement.blocking` block). A
profile also writes `.mcp.json` and `.cursor/mcp.json` (IDENTICAL
content), never overwriting — an existing target is skipped and omitted
from `files_written` (pinned: `init-profile-skips-existing-mcp-json`).
Idempotent re-init with the same key re-reads and validates the existing
config and IGNORES `--ticketing`/`--profile` entirely (created=false, no
files written). Paths are literal pathlib joins of the directory arg
(`i2/.decided/config.yaml`; default `.` yields `.decided/config.yaml`).

Exits: 0 created or idempotent; 1 `RepositoryKeyConflict` (`repository
already initialized with key 'RAC' (<path>); refusing to change it to
'OTHER' — established ID namespaces are never silently rewritten`) and
`MalformedRepositoryConfig`; 2 invalid key (`invalid repository key:
'bad' (expected 2-10 uppercase alphanumeric characters starting with a
letter, e.g. RAC)` — checked BEFORE the conflict, so `--key bad` on an
initialized repo is exit 2), not-a-directory, argparse choice errors.
`InvalidTicketingProvider`/`InvalidProfile` are UNREACHABLE from the CLI
(argparse choices fire first) and are not ported. KEY_RE quirk: Python
`$` matches before one trailing newline, so a key ending in `\n` passes
the syntax check — replicated in `valid_repository_key`.

Human created: `Initialized repository key <KEY>` / `Config: <path>` +
optional `Profile: <name>` + one `Wrote: <path>` per file. Idempotent:
`Already initialized: repository key <KEY>` (the verb carries the colon).
JSON: `{"schema_version":"1","repository_key":…,"config_path":…,
"created":…,"profile":null|…,"files_written":[…]}`.

## 4. `rac quickstart [directory] [--key KEY] [--type TYPE] [--json]`

Composes init + new: `--type` is a FREE string (no argparse choices,
validated by the template registry). ORDER-AWARE only for the
value-taking options: a missing `--key`/`--type` VALUE errors at its
position (`quickstart --type --version` exits 2) while the values
themselves are service-validated (`quickstart zzz --key bad --version`
prints the version — measured).

Check order is load-bearing (all pinned): `load_template(type)` FIRST
(exit 2, beats everything) → empty-corpus walk (`CorpusNotEmpty` exit 1:
`corpus already has artifacts (e.g. <path>); rac quickstart only
scaffolds an empty corpus — use \`rac new\` to add more` — beats a bad
key) → `init_repository` (bad key exit 2 / key conflict exit 1) →
`create_artifact` at `<dir>/rac/<type>s/first-<type>.md` (family is just
type+"s"). The empty check counts only entries classifying to a KNOWN
type — an unknown-documents-only corpus is still "empty" and scaffolds.
LANDMINE: the identity write lands BEFORE the starter-exists refusal, so
a squatted starter path exits 1 with `… already exists; rac new never
overwrites` (create's message) AND `.decided/config.yaml` freshly written —
pinned by `quickstart-err-starter-exists-config-written`. The same
`OutputPathExists` is exit 2 under `new` but exit 1 under quickstart
(refusal group).

Human: `Initialized|Using repository key <KEY>` (verb switches on the
idempotent init) / `Created <type> artifact: <path>` / `ID: <id>` /
blank / `Next: edit the TODO placeholders, then run: rac validate
<path>`. JSON: `{"schema_version":"1","repository_key":…,"config_path":…,
"created":…,"artifact":{"type":…,"path":…,"id":…}}` (nested artifact).

## 5. The one-time sharing prompt (init/quickstart, ADR-041)

After a successful NON-json init/quickstart, `_maybe_ask_usage_sharing`
runs only when stdin AND stdout are TTYs and no consent is recorded
(`consent_recorded()` — any persisted answer, including a decline).
Prompt bytes `\nShare anonymous usage to help shape Lore? [y/N] `;
`y`/`yes` (stripped, lowercased) → `opt_in()` + `Sharing on — one
anonymous daily ping. 'rac telemetry status' shows exactly what; 'rac
telemetry off' stops it.`; anything else / EOF → `decline()` silently.
Under the piped harness `isatty` is false on both sides, so the prompt
NEVER fires and no consent file appears — pinned by the two
`*-stdin-pipe-suppresses-prompt` cases (stdin `y\n` fed, `xdg-config/**`
captured empty). The interactive path itself is NOT byte-parity-testable
without a PTY harness mode (documented gap); the Rust port mirrors the
exact gate and bytes, and the answer classification is unit-pinned
(`commands::share_prompt_tests::share_answer_classification`).

## 6. `rac rename <old> <new> <directory> [--apply] [--top-level] [--json]`

THREE required positionals, directory LAST. NOT order-aware. Missing
positionals list all still-missing (`old, new, directory` / `new,
directory` / `directory`). Exit 2 only for argparse misuse and
not-a-directory; EVERY refused plan exits 1 — including `--apply` on a
refusal (nothing written, pinned by `rename-refusal-apply-writes-nothing`).

Refusal routing is split: human refusal → STDERR (`Rename <old> ->
<new>` / blank / `✗ Refused: <phrase>.`), JSON refusal → STDOUT (the
full plan with `ok:false` and a stable `reason` code: `old-ref-not-found`,
`old-ref-ambiguous`, `new-ref-collides` (target_path still set),
`new-ref-invalid`, `old-ref-filename-only`, `symlink-path`, or
`path-outside-root`).

Plan semantics (all pinned): `new_ref` is stripped, then must match
`^[A-Za-z][\w.-]*$` BEFORE any walk; `old_ref` resolves case-insensitively
through the relationship alias index to exactly one path; a `new_ref`
already naming ANOTHER artifact refuses (skipped when new==old
casefolded); identity precedence frontmatter `id` (only when it IS
old_ref) → `## ID` section first value → `spec.id_field` (dead — no spec
sets one, ported for fidelity) → filename-only refusal. LANDMINE: a
no-op rename (new == old) reaches the identity step, produces no change
(`new_line == old_line`), falls through every editable field, and
refuses `old-ref-filename-only` — pinned by
`rename-noop-same-id-refused-json`. Reference edits rewrite only lines
whose LEADING token equals old_ref (case-insensitive, whole-token — the
next char must not be alnum/`_-.`), preserving the list marker and
trailing text; a sibling line naming a different alias of the same
target is untouched. Edits sort by (path, line). The frontmatter `id:`
line match replicates `_FRONTMATTER_ID_RE` (optional matching quotes,
value excludes `'"#`, trailing `#` comment preserved) by hand
(`rename::frontmatter_id_line`, unit-pinned).

Dry-run human (exit 0): header, `=`*len(header), blank, `Target: <path>
(identity field: <field>)`, `<R> inbound reference(s), <I> identity edit
across <F> file(s).`, blank, per-file `  <path>` with `    L<n> ✗ <old>`
/ `    L<n> ✓ <new>` pairs, blank, `Dry run — pass --apply to write
these edits.`. Apply human: header, blank, `✓ Applied: <R> reference(s)
and <I> identity edit across <F> file(s).`. Apply JSON is the
RenameResult (no `edits` array). `--apply` replaces exact lines
(verified against `old_line`) and preserves the file's final-newline
shape; the plan `directory` echoes the argv verbatim (trailing slash
kept) while edit paths are walk-normalized. Mutation is confined to the
canonical corpus root (ADR-129): the target and every relationship-bearing
edit path must be a non-symlink regular path whose canonical destination is
under that root. A symlinked path or a path that cannot be resolved is a
whole-plan refusal with reason `symlink-path` or `path-outside-root`; the
human dry-run includes the offending path and JSON keeps it in
`target_path`. Apply repeats the checks immediately before each read and
replacement; Unix staging opens its final component with `O_NOFOLLOW` and
same-directory replacement uses the staged file. All affected files are
preflighted and staged before any replacement. Originals move to sibling
backups and a later failure triggers reverse-order rollback; the engine reports
`corpus restored` or an explicit `rollback incomplete` error. Successful
commits remove staging and backups; cleanup failures are reported rather than
silently ignored. Read-only discovery still yields symlinked Markdown files
for parity.

## 7. `rac migrate {metadata} <directory> [--dry-run] [--top-level] [--recursive] [--json]`

`target` is a choice-validated positional — ORDER-AWARE (`migrate
frobnicate x --version` exits 2 with `argument target: invalid choice:
'frobnicate' (choose from 'metadata')`; `migrate --version frobnicate x`
prints the version). Exit 2: bad choice, missing args (`target,
directory` / `directory`), not-a-directory, missing repository config
(`no repository identity found at or above <dir>; …` — the literal
directory arg). Exit 1: malformed config / id exhaustion. Exit 0 for
every completed run INCLUDING "nothing to migrate".

Per-file triage over the sorted walk: ANY frontmatter presence — valid,
malformed, or unterminated (`metadata is not None OR metadata_issues`) —
is `already-canonical` (migration never touches an existing envelope;
validation owns broken ones); no classified type → `skipped-unknown`;
else mint an id and (unless `--dry-run`) prepend the envelope bytes to
the UNTOUCHED original bytes. The issued-id set seeds from the repo ROOT
(config grandparent) index, uppercased, and dedupes within-run.

Human: optional bold `Dry run — no files were written.` + blank;
`Would migrate|Migrated <n> artifact(s):` with rows `  <path ljust w>
<id>  (<type>)` (or `<verb> 0 artifact(s) — nothing to migrate.`); a
`Skipped <n> unrecognized document(s):` block (+`  - <path>` rows) ONLY
when unknowns exist; blank; `<total> file(s): <m> migrated, <a> already
canonical, <s> skipped (unknown type).`. JSON: `{schema_version,
directory, recursive, dry_run, summary:{total_files, migrated,
already_canonical, skipped_unknown}, files:[{path, status, id, type}]}`
with id/type null unless migrated. The all-canonical re-run is fully
deterministic (no minting) — pinned RAW by `migrate-nothing-to-do-human`.

### 7.1 The hostile-markdown oracle-crash divergence (RAC-KXBPS7SRM6ZB REQ-002)

The `new`/`quickstart` id-collision and empty-check walks and migrate's
index seed all call `build_repository_index`; the oracle CRASHES (exit 1,
uncaught `TypeError: unhashable type: 'list'` from frontmatter
`_no_duplicates`) when any walked file carries a YAML mapping with a
list key (`rust/fuzz/pinned/oracle-crashes/unhashable-key/repro.md`).
The native walk is total: the hostile file yields its parse issues and
whatever identifier remains, and creation SUCCEEDS. Parity is impossible
by design for this class, so it is EXCLUDED from the case file and
pinned as the cargo test
`scaffold::tests::new_survives_hostile_markdown_in_the_walk` (cites
RAC-KXBPS7SRM6ZB REQ-002).

## 8. Known divergences (stderr-only / unreachable; exit codes match)

- `MalformedRepositoryConfig` invalid-YAML reasons embed PyYAML's
  multi-line parser diagnostic in the oracle; the engine embeds its
  bounded loader's problem text. Prefix (`rac: malformed repository
  config <path>: invalid YAML: `) and exit 1 match; the tail is stderr
  only, never byte-refereed (`init-err-malformed-config`).
- Write-failure paths (`create_artifact`/`init` write, migrate rewrite):
  the oracle lets the `OSError` escape as a traceback, exit 1; the
  engine prints a readable `rac: …` line, same exit.
- `rename --apply` on a stale plan: the oracle's uncaught `ValueError`
  traceback (exit 1) vs the engine's `rename: stale plan for <path> line
  <n>: …` stderr line, same exit. Unreachable in a single CLI run.
- `rename` reads raw file text strictly in the oracle
  (`read_text(encoding="utf-8")`, tracebacks on invalid UTF-8); the
  engine decodes leniently — same class as the walk's lossy read.
- The interactive sharing prompt (§5) is real-TTY only.

## 9. Parity coverage

`rust/parity-cases-closure.json` (all green oracle-vs-oracle and
oracle-vs-rust): 14 `new` cases (the two B0 `closure-smoke-new-*`
written-tree cases plus 12: roadmap/prompt/design template trees with
verbatim `./` and `//` path echoes, bad type / exists / missing parent /
no config with captured-tree no-write proofs, both missing-positional
shapes, extra-positional-writes-nothing, version ordering both ways);
20 `init-` (created human/json, default-dir `.decided/config.yaml` shape,
inline `--key=`, idempotent + profile-ignoring idempotent, default and
enterprise+ticketing profile trees, existing-`.mcp.json` skip, key
conflict, invalid-key-beats-conflict, malformed config, not-a-directory,
both bad choices, choice/version ordering both ways, `--key` missing
value, extra-positional, piped-stdin prompt suppression); 17
`quickstart-` (happy human/json, default dir, roadmap family, `Using`
verb, unknown-only corpus scaffolds, corpus-not-empty, starter-exists
with config-written capture, bad type/key with no-write captures, both
precedence pins (type-beats-corpus, corpus-beats-bad-key), key conflict,
not-a-directory, version-vs-missing-value ordering, prompt suppression);
19 `rename-` (dry-run human/json, apply trees human/json, `## ID`
section identity, case-insensitive resolution, no-op refusal, all five
refusal reasons as JSON + the human-stderr variant, refused-apply
no-write capture, `--top-level` recursive:false, trailing-slash echo,
not-a-directory, missing args, version-after-flags); 16 `migrate-`
(dry-run and write trees human/json with id masking, the deterministic
nothing-to-do raw-byte case, top-level vs `--recursive`, ljust
alignment, no config, not-a-directory, bad target + ordering both ways,
both missing-arg shapes, extra-positional no-write capture).
