# Governance: the enforcement policy & `decided gate`

`decided gate` is RAC's single enforcement entry point. It runs validation,
relationship integrity, and review over a corpus, then classifies every finding
as **blocking** or **advisory** under the corpus *enforcement policy*. One
command, one exit code, one SARIF document — so a pull-request gate carries the
whole RAC contract as a single required check.

The policy is governed, not hardcoded. A repository declares which finding
classes block the gate in its committed `.decided/config.yaml`, so the same policy
travels with the corpus and applies identically in the editor and in CI. The
decisions behind this are
[ADR-049](https://github.com/itsthelore/asdecided-core/blob/main/decisions/decisions/adr-049-enforcement-is-the-product.md)
(enforcement is the product) and
[ADR-063](https://github.com/itsthelore/asdecided-core/blob/main/decisions/decisions/adr-063-non-python-clients-are-thin.md)
(policy lives in the corpus, not in any consumer).

## Running the gate

```bash
decided gate decisions/              # human summary; exit 0 if nothing blocking, else 1
decided gate decisions/ --json       # stable JSON contract (ADR-007)
decided gate decisions/ --sarif      # one SARIF 2.1.0 document over all findings
decided gate decisions/ --top-level  # do not recurse into subdirectories
```

The exit code is the only enforcement signal: `0` when nothing is blocking, `1`
when at least one finding is blocking. Advisory findings are reported (and
annotated in SARIF) but never fail the gate.

## The `enforcement:` policy

Add an optional top-level `enforcement:` section to `.decided/config.yaml`. It maps
**finding codes** (the stable `[code]` shown in `decided validate`, `decided review`, and
`decided relationships --validate` output) to an enforcement class:

```yaml
repository_key: RAC

enforcement:
  blocking:                          # force these codes to fail the gate
    - missing-recommended-sections
  advisory:                          # downgrade these to annotate-only
    - relationship-target-superseded
  off:                               # drop these findings entirely
    - stale-corpus
```

- **`blocking`** promotes a code so any matching finding fails the gate.
- **`advisory`** downgrades a code so matching findings annotate but do not fail.
- **`off`** suppresses matching findings entirely — they do not appear at all.

**Precedence** (so a code listed in more than one set is deterministic): `off`
wins, then `blocking`, then `advisory`, otherwise the finding keeps its default
class. The section is additive and offline; an absent `enforcement:` section is a
pure no-op, and the gate's default verdict is then exactly
`validate.ok AND relationships.ok AND review.ok` — the strict v0.21.13 behaviour.

> YAML note: the bare word `off` is a boolean in YAML 1.1, but `rac` accepts it
> as the suppression key without quotes — write `off:` directly.

### Default classifications

With no policy, each finding carries a default class chosen so the gate is
`ok` exactly when validate, relationships, and review all pass:

| Source | Default classification |
| --- | --- |
| `validate` — `error` severity | **blocking** |
| `validate` — `warning` / `info` (incl. OKF) | advisory |
| `relationships` — every finding | **blocking** |
| `review` — priority 1–2 (invalid artifact, broken relationship) | **blocking** |
| `review` — priority 3+ (unknown artifact, missing recommended, stale) | advisory |

A policy entry only *changes* a default; it never invents findings.

## Worked example: downgrade a superseded reference

Suppose a live roadmap references a decision the team has retired. By default that
is a blocking `relationship-target-superseded` finding, so the gate fails:

```text
$ decided gate decisions/
...
Blocking:   1
  ✗ decisions/roadmaps/v1.md
      [relationships] relationship-target-superseded: related_decisions: adr-002 — target is superseded
✗ Gate failed — 1 blocking finding(s).
$ echo $?
1
```

Downgrade just that code in `.decided/config.yaml`:

```yaml
enforcement:
  advisory:
    - relationship-target-superseded
```

The finding still surfaces — now as advisory, and it still annotates the pull
request in SARIF — but the gate passes:

```text
$ decided gate decisions/
...
Blocking:   0
Advisory:   1
  ! decisions/roadmaps/v1.md
      [relationships] relationship-target-superseded: related_decisions: adr-002 — target is superseded
✓ Gate passed — nothing blocking.
$ echo $?
0
```

The reference is still visible to reviewers; what changed is whether it *blocks*
the merge — exactly the governed knob a maintainer wants.

## Fleet readiness — one policy across many repositories

Enforcement only standardises an organisation when *which findings are blocking*
is set centrally, not re-litigated per repository. Because the policy is plain
data in `.decided/config.yaml`, a team can standardise it across a fleet:

- **Commit a shared `enforcement:` block.** Keep the canonical policy in one
  place (a platform repo, a template repo, or a copier/cookiecutter template) and
  have every RAC repository carry the same `.decided/config.yaml` enforcement section.
  Each repository's `decided gate` then makes the same blocking-versus-advisory
  decisions without per-repository edits.
- **Tighten centrally, roll out by sync.** Promoting a code from `advisory` to
  `blocking` in the shared block, then syncing the file, tightens the gate across
  every repository at once — the change is visible in version control and applies
  identically in the editor and in CI.
- **Adopt warnings-first, then ratchet.** A new team can start with broad
  `advisory`/`off` entries to get the gate green on a legacy corpus, then move
  codes back to `blocking` one at a time as the corpus is cleaned up — the same
  ratchet the severity overrides ([ADR-053](https://github.com/itsthelore/asdecided-core/blob/main/decisions/decisions/adr-053-validation-severity-overrides.md))
  offer for `decided validate`, now spanning the whole gate.

Because the same file governs both the editor and the PR action, a fleet-wide
policy change lands in one edit and is enforced everywhere the corpus is checked.

## Agent integration: context supply and enforcement

AI coding agents are the place a settled decision is most likely to be quietly
re-litigated, because the agent never sees the corpus. RAC integrates with agents
through two deterministic, engine-owned channels and enforces with structural
validation — **not** a semantic verdict and **not** a cross-platform interceptor
([ADR-067](https://github.com/itsthelore/asdecided-core/blob/main/decisions/decisions/adr-067-agent-integration-boundary.md)):

- **Context supply.** `decided export --agent-rules` generates committed,
  drift-guarded rules files (`CLAUDE.md`, `AGENTS.md`, `.cursor/rules`,
  `.github/copilot-instructions.md`) plus the `lore` MCP read tools. This reaches
  *every* agent — including Copilot — with zero per-developer setup.
- **Post-edit enforcement.** The same structural diagnostics fire on
  agent-written files exactly as on human edits: the editor's save-time
  diagnostics, `decided validate`, and the `decided gate` PR check.

No platform exposes a hook to inspect-and-veto a proposed agent edit before it
lands — with **one** exception.

### Claude Code pre-edit hook

Claude Code's `PreToolUse` hook is the single platform seam that permits a real
pre-edit veto. The RAC VS Code/Cursor extension can **generate** an opt-in hook
for it (command: **"RAC: Enable Claude Code pre-edit hook"**); the extension is
not itself the interceptor. The generated hook:

- lives at `.claude/hooks/rac-preedit.py` and is registered in
  `.claude/settings.json` under `hooks.PreToolUse` (merged in without clobbering
  existing settings);
- fires on `Write`/`Edit`/`MultiEdit`, acts only on Markdown files under `decisions/`,
  and pipes the proposed content to `decided validate - --corpus decisions/`
  ([CLI reference](cli.md#corpus-aware-single-document-validation-corpus));
- **blocks** the edit (exit 2, with the finding) only on a *structural* finding —
  a reference to a retired or missing decision, or a malformed artifact;
- **fails open** on any internal error (missing `rac`, unreadable file): a hook
  fault never blocks a developer, only a real reported contradiction.

It is **opt-in and Claude-Code-specific**. All validation stays in `rac` — the
hook computes nothing
([ADR-063](https://github.com/itsthelore/asdecided-core/blob/main/decisions/decisions/adr-063-non-python-clients-are-thin.md)).
Disabling it (**"RAC: Disable Claude Code pre-edit hook"**, or deleting the
registration) falls back cleanly to the post-edit diagnostics above. RAC makes no
claim of intercepting Copilot inline suggestions or Cursor agent edits — no
platform API exists for that, so those clients rely on the post-edit guard.

## See also

- [Validation](validation.md) — severity overrides (ADR-053) and SARIF.
- [Security posture](security.md) — the offline guarantee behind the gate.
- [Repository Workflow](repo-workflow.md) — `decided init` and `.decided/config.yaml`.
- [CLI Reference](cli.md) — `decided gate` flags and exit codes.
