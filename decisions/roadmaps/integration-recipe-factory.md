---
schema_version: 1
id: RAC-KVV60ZNYKXQT
type: roadmap
---
# Integration Recipe Factory

## Status

Achieved

Delivered across all three initiatives (PRs #309, #310): the recipe authoring
template and contract, the verification gate, and the prioritised harness
backlog — proven with four recipes authored from the template (Zed, Windsurf,
Cline, opencode), each engine-half smoke-tested and carrying its
`verify against` marker until harness-connected verification. Zero engine diff,
as designed — every recipe is a documentation unit. The programme's boundary
constraint held throughout: **every recipe is context-supply and post-edit CI
enforcement (ADR-067), never pre-edit interception.** Was the rank-6 Tranche A
item of the deterministic-substrate programme (the *harness* half of the
integration story, beside `corpus-export-to-rag-backends`); execution tracked in
GitHub (ADR-093).

## Context

RAC already meets coding agents on two surfaces it does not own: a generated
`AGENTS.md` / `CLAUDE.md` an agent reads for instructions (the "push"), and the
`lore` MCP server an agent connects to for live retrieval (the "pull"). Because
both are standard surfaces, connecting a new harness is documentation, not
engine work — a worked `examples/<client>/` setup (README plus a sample config)
and a subsection in `docs/mcp.md`. The repository already carries this pattern
for Amp, Claude Code, Codex, Copilot, Cursor, and Omnigent.

Today each new harness is added ad hoc. The same shapes recur — the same
`command: rac, args: [mcp, --root, .]` block in three config dialects (JSON,
TOML, YAML), the same push/pull/enforcement structure in each README, the same
"verify with the grounding demo" close — so the work is a factory waiting to be
named. The agent-tool landscape is wide and moving (Windsurf, Cline, Zed,
Gemini CLI, Goose, Continue, JetBrains AI, and more), so footprint here grows by
*repeating a cheap, on-thesis recipe*, not by building per-harness code.

This is the highest-leverage footprint lever precisely because it changes
nothing about RAC's boundary: every recipe is context-supply and post-edit
enforcement (ADR-067), the agent-ready surface RAC already committed to
(ADR-008), with non-Python clients staying thin consumers of the contract
(ADR-063). RAC stores nothing new and serves nothing new (ADR-024).

## Outcomes

- Adding a new harness integration is a **repeatable, contract-shaped task**: a
  documented recipe template plus a checklist produce a consistent
  `examples/<client>/` setup and `docs/mcp.md` subsection, with no engine change.
- **The set of supported harnesses grows and is named explicitly.** A reader can
  see, in `docs/ecosystem.md`, exactly which harnesses have a verified recipe —
  never a vague "works with any MCP client".
- Every recipe carries the same three surfaces — push (`AGENTS.md`), pull (`lore`
  MCP), and enforcement (CI gate, ADR-067 / ADR-065) — so the boundary is
  restated identically everywhere and never drifts into pre-edit interception.
- A recipe is listed in `docs/ecosystem.md` **only after it is verified against a
  released engine version**, honouring that file's existing real-and-verified
  rule; unverified recipes ship with the `verify against <client> <version>`
  marker in `docs/mcp.md` and stay off the ecosystem table until smoke-tested.

## Initiatives

### Initiative 1 — A recipe template and authoring contract

Extract the recurring shape into a reusable contract: the push/pull/enforcement
README skeleton, the three config dialects (JSON / TOML / YAML) for the same
`lore` server invocation, and the standard "verify with `examples/guide/`" close.
Authoring a new recipe becomes filling the template, the same way `rac new` makes
an artifact from a template (ADR-021).

### Initiative 2 — A prioritised harness backlog

Maintain a named, prioritised list of candidate harnesses (e.g. Windsurf, Cline,
Zed, Gemini CLI, Goose, Continue, JetBrains AI), each a single
`examples/<client>/` unit of work. Priority follows real adoption signal, not
completeness; the list is honest about what is shipped versus candidate.

### Initiative 3 — A verification gate before ecosystem listing

Each recipe is smoke-tested against the released engine before its
`docs/ecosystem.md` row is added. Until then it lives as documentation with the
`verify against <client> <version>` marker (the convention already in
`docs/mcp.md`). This keeps the ecosystem table trustworthy and the boundary
between "documented" and "verified" explicit.

## Success Measures

- A contributor can produce a new, structurally consistent `examples/<client>/`
  recipe from the template and checklist alone, without reading another recipe.
- `docs/ecosystem.md` names every harness with a verified recipe; each row is
  real and verified, and no row is added before smoke-test.
- The grounding demo (`examples/guide/`) is the verification close for every
  recipe, so each is proven against the same engine behaviour.
- New recipes land with zero `rac-core` engine diff.

## Assumptions

- `rac export rac/ --agent-rules` and the `lore` MCP server remain the two stable
  integration surfaces (ADR-008, ADR-030, ADR-031), with the export a stable
  additive contract (ADR-007, ADR-063).
- The "MCP server + agent-instructions file" pattern remains the de-facto way
  harnesses consume external context, so one recipe shape serves most targets.
- Adoption signal justifies which harnesses are worth a verified recipe.

## Risks

- **Harness sprawl.** Chasing every MCP client is unbounded. Mitigation: ship
  verified recipes by adoption signal; the template is the product, the backlog
  is explicitly prioritised, not exhaustive.
- **Stale config dialects.** A harness changes its config format and a recipe
  rots. Mitigation: the `verify against <client> <version>` marker and the
  ecosystem-listing gate keep claims dated and verified.
- **Boundary drift.** A harness offers a pre-edit hook and a recipe is tempted to
  use it as enforcement. Mitigation: the enforcement section is fixed by ADR-067
  — context-supply and post-edit CI, restated identically in every recipe.

## Related Decisions

- ADR-007
- ADR-008
- ADR-021
- ADR-024
- ADR-030
- ADR-031
- ADR-063
- ADR-067

## Related Roadmaps

- corpus-export-to-rag-backends
- deterministic-substrate
- growth-programme

## Related Requirements

- rac-recipe-authoring-contract
- rac-recipe-verification-gate

## Related Tickets

- itsthelore/asdecided-core#248
