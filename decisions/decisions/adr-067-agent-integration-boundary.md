---
schema_version: 1
id: RAC-KV80WX94GY8A
type: decision
---
# ADR-067: Agent Integration is Context-Supply and Post-Edit Enforcement, Not Pre-Edit Interception

## Context

RAC's value — "enforcement is the product" (ADR-049) — is undercut when the AI
coding agent (GitHub Copilot, Cursor, Claude Code), not the human, re-litigates
settled decisions, because the agent never sees the corpus. A design council and
a focused spike examined how to get RAC's recorded decisions and enforcement to
agents. Two temptations surfaced, and both fail RAC's invariants or the platform
reality:

1. Have the engine compute a semantic verdict — "this proposed change contradicts
   ADR-NNN." That requires an LLM or a brittle heuristic inside `rac`, breaking
   offline, deterministic, no-telemetry operation, and contradicting RAC's core
   principle of structural validation, not semantic scoring.
2. Have the editor intercept an agent's proposed edit before it lands. No
   editor/agent platform exposes an extension hook to inspect-and-veto a proposed
   agent edit (Copilot inline suggestions, Cursor agent edits, the VS Code LM
   API). Promising it would roadmap the impossible.

The thin-client boundary (ADR-063) already says clients consume `rac` / the
`lore` MCP server (ADR-030) and never reimplement the engine. The relevant
surface already exists: the `lore` read tools, `rac validate -` (stdin
validation), and status-aware relationship validation (a reference to a
superseded/retired decision is already flagged).

## Decision

RAC integrates with AI agents through two deterministic, engine-owned channels,
and enforces through post-edit structural validation:

- **Context supply.** `rac` generates a drift-guarded agent-context projection —
  rules/context files (`CLAUDE.md`, `AGENTS.md`, `.cursor/rules`,
  `.github/copilot-instructions.md`) emitted from live (Accepted, non-superseded)
  artifacts — and the `lore` MCP read tools the agent queries.
- **Post-edit enforcement.** The existing structural diagnostics (which fire on
  agent-written files exactly as on human edits), a save-time backstop, and the
  PR gate, plus a generated client-specific pre-edit hook (Claude Code
  `PreToolUse`) where that single platform permits a real veto.

The boundary is firm. RAC will **not**:

- ship any engine surface (CLI verb or MCP tool) that returns a semantic verdict
  on whether proposed natural-language text contradicts a decision's meaning. The
  engine asserts *which live decisions bind a change* (deterministic retrieval +
  structural reference validity); it does not assert that a change *is wrong*.
  Semantic entailment stays in the consuming agent, operating on engine-supplied
  decisions.
- promise generic pre-edit interception of agent suggestions, because no platform
  exposes the hook. Integration is context-supply + post-edit enforcement.

Generation lives in `rac` (Core); the extension orchestrates, registers MCP,
watches for drift, and presents — it computes nothing (ADR-063).

## Code Constraints

```yaml
version: 1
eligibility: eligible
reason: "Context projection and post-edit enforcement have deterministic engine and CI anchors."
rules:
  - id: core-keeps-agent-rules-generator
    kind: require_pattern
    path_glob: "rust/rac-engine/src/agent_rules.rs"
    pattern: 'pub fn generate_agent_rules'
    message: "Core must retain the engine-owned agent-context projection."
  - id: agent-rules-has-no-model-client
    kind: forbid_import
    path_glob: "rust/rac-engine/src/agent_rules.rs"
    pattern: '^(reqwest|hyper|ureq|openai|anthropic)(::|$)'
    message: "Agent-context generation must remain deterministic and local."
  - id: core-keeps-post-edit-sentry
    kind: require_pattern
    path_glob: ".github/workflows/pr-checks.yml"
    pattern: 'decided sentry decisions'
    message: "Core pull requests must retain deterministic post-edit code enforcement."
```

## Consequences

The engine stays deterministic, offline, no-telemetry, and authoritative — its
authority comes precisely from refusing to assert facts it cannot derive
deterministically. No model enters `rac`. Coverage is universal at the context
layer (committed, version-controlled rules files reach every agent including
Copilot, with zero per-developer setup) and best-effort at the live-query layer
(MCP reaches Claude Code / Cursor power users). The "guard" is honestly scoped:
post-edit diagnostics + save-gate + PR gate, plus a generated Claude Code
pre-edit hook — not a cross-platform interceptor that cannot be built.

Trade-off: RAC does not catch a contradiction the agent's own model fails to
reason about; it supplies the binding decisions and lets the agent judge. We
accept this over shipping a brittle or semantic engine verdict that would erode
trust — a guard that cries wolf is worse than none. Generated rules files are a
derived projection carrying a provenance hash and a `--check` staleness gate, so
they cannot silently rot.

## Status

Accepted

## Category

Architecture

## Alternatives Considered

- **Engine computes the contradiction verdict** (semantic match/score in `rac`).
  Rejected: needs an LLM or brittle heuristic in the engine — breaks
  offline/deterministic/no-telemetry and makes `rac` a confident liar; violates
  structural-validation-not-semantic-scoring.
- **Editor intercepts agent edits pre-write.** Rejected: no platform API exists
  (Copilot inline, Cursor agent edits, VS Code LM API); it would promise the
  impossible.
- **MCP-only delivery.** Rejected: MCP is per-developer opt-in configuration that
  reaches power users (who least need it) and barely reaches Copilot (where the
  risk concentrates); committed generated rules files reach everyone with zero
  setup and are reviewable in a PR.
- **Inline the whole corpus into the rules files.** Rejected: bloated context
  exceeds the agent's attention budget and rots; rules files carry distilled bans
  and closed-decision pointers and point at the MCP tool for ground truth.

## Related Decisions

- adr-049
- adr-063
- adr-030
- adr-007

## Related Roadmaps

- v0.21.15-agent-context-generation
