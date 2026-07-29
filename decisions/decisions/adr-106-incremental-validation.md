---
schema_version: 1
id: RAC-KWSH9J2S7QB1
type: decision
---
# ADR-106: Incremental Directory Validation

## Context

`rac validate DIR` re-parses and re-validates every artifact on every run.
On the `rebuild-scale` reference corpus this is the same cost whether one file
changed or none did — incremental validation is identical to a full validation
(~77 s at 100k files, extrapolating to ~13 min at 1M). The `rebuild-scale`
performance gate asks for a re-validate after a ~1,000-file changeset in under
5 s, independent of corpus size. The Movement-B performance lens (v2 §3) isolates
the fix and, critically, corrects two things the v1 lens got wrong.

**Directory validation is a pure per-file computation.** The core-validate rebuild
brief (§4) establishes that a file's `FileValidation` is a pure function of
`(file bytes, resolved config)`: `validate(product, ticketing_provider,
artifact_type)` depends only on the parsed product (a pure function of the file's
bytes) and two repository-wide config scalars (the severity overrides and the
ticketing provider), and OKF conformance is likewise per-file (each finding
depends only on that entry's type and filename). There is **no cross-file layer**
in `rac validate DIR`: duplicate-identifier detection, relationship-target
resolution, and supersedes-cycle detection all live in the relationships
subsystem (`rac relationships --validate`, `rac gate`), not here. So a
changeset-bound re-validate of `rac validate DIR` needs only a per-file result
cache — no corpus-global index, no refindex, no cross-file join.

**The cache key must cover the ancestor-walked config.** The core-validate audit
(v2 §3.1) sharpens "pure function of `(bytes, config)`": *config* means
`find_config_file(directory)` — the nearest `.rac/config.yaml` at **or above** the
target, with relative starts bound to the process CWD via `.resolve()`. A
directory-local fingerprint would miss an ancestor-config edit and serve stale
severity verdicts. The key must therefore be `content_hash(file) ×
fingerprint(resolved-config-path + its bytes)`.

**The <5 s gate is de-confounded from changed-set detection (v2 §3.3, F3).** The
gate corpus is generated outside any git tree, so a one-shot `rac validate` must
discover the changed set by a stat-manifest scan — an O(files) `stat` pass
(1–10 s warm to ~100 s+ cold at 1M) *before* any recompute. Recompute (parse +
per-file validate the changed set + reassemble) is O(changed) and flat in N; stat
detection is O(files) and is the honest floor. The two are separate line items and
are never conflated.

This revises nothing in the default path. With the cache off, today's
`validate_directory` runs byte-for-byte unchanged; the incremental mode is opt-in
behind the same `--cache` flag `rac mcp` already uses (ADR-099), not activated by
the presence of `RAC_CACHE_DIR`.

## Decision

`rac validate DIR --cache` maintains a **per-file validation-result cache** keyed
by `content_hash × config-fingerprint`, so a re-validate re-parses and
re-validates only the changed set and reuses every unchanged file's result
verbatim. It is opt-in (`--cache`, off by default), and its output —
`DirectoryValidation`, and therefore the human, `--json`, and SARIF renderings and
the exit code — is **byte-identical** to the uncached `validate_directory` run for
the same corpus and config.

**Detection reuses the stat-manifest rung (ADR-105).** The changed / added /
removed set is found by `services.freshness.stat_scan` — the exact
`find_markdown_files` walk scope, `stat` each file for `(size, mtime_ns)`, and
content-confirm (read + hash) only the files whose stat proxy changed or that are
new. Enumeration makes add / remove / rename staleness-free (the path set is
ground truth); the manifest scan is extracted into a shared helper so the CLI and
the long-lived server tracker share one differ with no behaviour change to either.
The honest cost is stated, not hidden: O(files) `stat` (~1–10 s warm at 1M), so
the recompute half is the flat, N-independent, B4-owned claim, while end-to-end
one-shot detection is stat-floor-bound on a git-less corpus — the `<5 s` gate is
met on the recompute term and, end-to-end, up to roughly the warm-cache 0.5–1M
point without a resident-watcher or git oracle (v2 §3.3). No git dependency is
built; the stat rung works everywhere.

**Recompute is per-file, keyed by `(content_hash, config-fingerprint)`.** A
changed file is re-parsed, re-classified, and re-validated (`validate` with the
repository's overrides and ticketing provider applied); an unchanged file reuses
its cached `artifact_type`, `status`, and `Issue` list. No `Issue` message or line
embeds the file path, so the cached result is path-free and a rename (same bytes,
new path) reuses it unchanged with the current path re-attached at assembly time.
OKF conformance depends on `(artifact_type, current basename)` — the reserved-
filename check keys on the current name, which a rename changes while the content
hash does not — so it is recomputed each run from the cached artifact type and the
current path (no re-parse), never cached by content hash. The
`config-fingerprint` is the SHA-256 of the resolved `find_config_file(directory)`
path plus its bytes (a sentinel when absent): an ancestor-config edit, or the same
tree validated from a CWD that resolves a different governing config, changes the
fingerprint and invalidates every cached result.

**The cross-file transition classes are out of scope for this bundle, by
construction.** The performance lens frames incremental cross-file validation as a
declared-reference index (the refindex) over all edge target texts plus transition
classes T1–T8 (a not-found reference becoming resolved by an added file; a
duplicate identifier appearing; a referenced file removed; a supersedes cycle
created or broken; a target's status flipped), with a partial-global fallback when
a hub id's reference fan-in is large. **That mechanism belongs to the
relationships subsystem's future incremental bundle, not here**, because
`rac validate DIR` emits none of those findings: they are computed by
`validate_relationships` / `validate_document_against_corpus`, which this bundle
does not modify and does not call. B4 therefore does **not** build or persist a
refindex or an identifier multimap — an unconsumed, only-round-trip-tested index
is exactly the speculative infrastructure the rebuild discipline rejects, and
computing it would add relationship-extraction cost `rac validate DIR` never paid.
The refindex / T1–T8 design is recorded here as the design of record for that
future bundle (per v2 §3.2); when it lands it will reuse this bundle's per-file
layer unchanged and add its own cross-file layer.

**Persistence follows the ADR-104 store discipline.** The per-file results are
written as one length-prefixed binary segment per corpus root under the cache
directory (`validate/v1/{root-key}.vseg`), carrying each file's `(size, mtime_ns,
content_hash)` stat proxy in the same row so the store doubles as the freshness
manifest and no second on-disk structure is needed. Reads are fixed struct reads —
no `pickle`, `eval`, or `marshal`, so a hostile or truncated file can at worst
raise and become a miss, never execute. Every read is bounds-checked and the
segment's declared length must match the file exactly, so truncation or garbage is
caught on open and fails closed. The config-fingerprint is stored in the header
and checked on open: a mismatch is a miss. Writes are atomic (temp file, then
`os.replace`). Every failure mode — a missing store, a corrupt or truncated
segment, a config-fingerprint mismatch, an unwritable cache directory — degrades to
a full recompute, so enabling the cache can only change latency, never the answer
(ADR-080). Invalidation is content-addressed per file; there is deliberately no
corpus-level key and no O(bytes) whole-corpus re-hash — that is the point.

**Accepted staleness (S5), unchanged from ADR-105.** The stat rung diffs on
`(size, mtime_ns)`, so the single missable case is an in-place rewrite that
preserves **both** size and mtime_ns (a backdated same-length rewrite, a byte
restore). Add / remove / rename are never at risk — enumeration detects them from
the path set. `--verify` forces a full content re-hash and catches even S5. This is
the same accepted trade ADR-105 records at its S5; it is named and pinned by a
test, not silent staleness.

**The scorecard split is stderr-only and opt-in.** When incremental mode runs it
prints nothing extra — the frozen stdout bytes do not move. The performance harness
needs detection-vs-recompute visibility, so when `RAC_TIMING` is set one line is
written to **stderr**: `rac-timing: detect_ms=X recompute_ms=Y files_changed=N`.
It is absent by default and never touches stdout, so no frozen output byte changes.

This decision builds on ADR-099 (the opt-in `--cache` derived-index cache),
ADR-104 (the binary-segment store discipline reused for the results store), and
ADR-105 (the stat-manifest freshness rung reused as the detection differ). It does
not supersede or revise any of them; the default path is untouched.

## Consequences

`rac validate DIR --cache` re-validate cost drops from O(corpus) to O(changed set)
on the recompute term: after a ~1,000-file changeset only those files are re-parsed
and re-validated, and the recompute is flat in corpus size — the B4-owned gate
claim. The unchanged-file reuse is proven by a counting seam on `validate()`
(cold: every file; warm no-change: zero; after one edit: one). Output is
byte-identical to the uncached run across no-change, edit, add, remove, and rename,
so the human / JSON / SARIF renderings and the exit code are preserved exactly, and
the same holds across the file mutations the lens frames as cross-file transitions
— because `rac validate DIR` emits no cross-file findings, both paths agree on every
one, which is the guarantee the per-file cache must uphold.

The honest limits are on the record. **Detection is the stat floor**, O(files)
`stat`, so end-to-end one-shot incremental validate on a git-less corpus meets `<5
s` only up to roughly the warm-cache 0.5–1M point and not at 10M without a resident
watcher or git oracle — the recompute half is flat, the detection half is not, and
the harness reports them separately with the active mode named. **S5** — an in-place
rewrite preserving both size and mtime_ns — is the single accepted miss, caught by
`--verify` and pinned by a test that documents the trade. **The cross-file
transition classes (T1–T8) are not delivered here**; incremental relationship
validation (`rac relationships --validate`, `rac gate`) remains O(corpus) until the
relationships subsystem's own incremental bundle builds the refindex this ADR
records as its design of record. The results store adds a small, disposable
per-root cache file; deleting it costs only latency.

The risk is a stale cached result silently served. It is mitigated structurally:
content addressing per file means only an S5 rewrite can defeat detection, and the
config-fingerprint gate invalidates the whole cache on any change to the governing
`.rac/config.yaml` — including an ancestor edit, the trap the audit named. A
corrupt store is a miss, not a wrong answer, and the corruption / truncation /
config-mismatch cases are each pinned by a test.

## Status

Accepted

## Category

Architecture

## Alternatives Considered

### Build the refindex and transition classes (T1–T8) in this bundle

Persist a declared-reference index over all edge target texts and an identifier
multimap, and recompute the cross-file transition classes changeset-bound, as the
performance lens describes for incremental cross-file validation. Rejected for this
bundle because `rac validate DIR` — the command B4 owns — emits no cross-file
findings: duplicate-identifier, relationship-resolution, and cycle findings are
produced by the relationships subsystem, which this bundle must not modify.
Building and persisting a refindex here would add relationship-extraction cost that
`rac validate DIR` never paid and would leave an index with no consumer and only a
round-trip test — the speculative, untested-serialization infrastructure the
rebuild discipline rejects. The mechanism is recorded here as the design of record
for the relationships subsystem's future incremental bundle instead, which will
reuse this bundle's per-file layer unchanged.

### Cache by a directory-local `.rac/config.yaml` fingerprint

Key per-file results on `content_hash × fingerprint(directory/.rac/config.yaml)`.
Rejected: the governing config can live in an **ancestor** of the validated
directory (`find_config_file` walks upward), so a directory-local fingerprint
misses an ancestor-config edit and serves stale severity verdicts — the exact trap
the core-validate audit named. The fingerprint hashes the resolved
`find_config_file(directory)` path and bytes instead, so an ancestor edit or a
different-CWD resolution invalidates the cache.

### A corpus-level content-hash key (ADR-099's model)

Key the whole result set on `corpus_content_hash` and reuse it wholesale under an
unchanged key. Rejected for the incremental gate: `corpus_content_hash` is an
Ω(bytes) read of every file, so it reintroduces the O(bytes) whole-corpus toll the
incremental work exists to remove, and any single change invalidates the entire
result set rather than one file. Content addressing per file is what makes the
recompute changeset-bound; the corpus-level key is retained only where a whole-
corpus derived structure genuinely needs it (the mmap store, ADR-104).

### Trust file mtime as the invalidation signal

Reuse a file's result whenever its mtime is unchanged. Rejected for the same reason
ADR-032 and ADR-105 reject it: mtime alone is an unreliable invalidation signal
(save-in-place, same-second rewrites). `(size, mtime_ns)` is only a cheap prefilter
that selects which files to content-confirm; content hashing remains the truth, and
the one case the prefilter alone can miss (S5) is named, pinned, and caught by
`--verify`.

## Code Constraints

```yaml
version: 1
eligibility: eligible
reason: "Incremental validation and the explicit content-confirmation floor have stable command and regression-test anchors."
rules:
  - id: engine-keeps-incremental-directory-validation
    kind: require_pattern
    path_glob: "rust/rac-engine/src/commands.rs"
    pattern: 'pub fn validate_directory_incremental'
    message: "The native engine must retain its incremental directory-validation path."
  - id: verify-keeps-stat-preserving-rewrite-coverage
    kind: require_pattern
    path_glob: "rust/rac-engine/tests/incremental_validate.rs"
    pattern: 's5_stat_preserving_rewrite_is_the_accepted_miss_and_verify_catches_it'
    message: "The verify floor must remain pinned against stat-preserving rewrites."
```
