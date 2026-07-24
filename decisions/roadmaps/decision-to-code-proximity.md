---
schema_version: 1
id: RAC-KVTRP9G4CN2N
type: roadmap
---
# RAC — Decision-to-Code Proximity

## Status

Achieved

Delivered across all three initiatives (epic itsthelore/asdecided-core#273):
decisions declare validated code scope via `## Applies To` (#274); a
deterministic path→decisions lookup answers over the `rac decisions-for` CLI
and the additive `find_decisions` MCP `path` argument (#275); and the Explorer
consumes the same shared reader through `/decisions-for` while the drift-gate
handoff is recorded in the `code-scope-consumption` design (#276). Graduated as
the rank-1 Tranche A item of the deterministic-substrate programme; its
constraint pattern held throughout — **proximity references are declared and
validated, never inferred; drift findings are advisory before they are ever a
gate.** Execution was tracked in GitHub (ADR-093), a sub-issue per initiative.

## Context

A consistent finding across the research: at 20+ engineers, **discovery and
proximity-to-code — not capture — decide whether a decision corpus survives.**
Capture is solved and identical everywhere (Markdown-in-git). What separates the
tools that get read from the ones that rot is whether the record surfaces *where
the work happens*. ADRs "rot when stored away from the code, where engineers do
not look"; ThoughtWorks' rule is to keep decisions "in source control… in sync
with the code"; Backstage's ADR plugin attaches records to the service they affect
via a `backstage.io/adr-location` annotation so they "sit next to the services
they affect."

Lore's artifacts live in `rac/`, validated and linked to *each other*, but they
are not linked to the **code or components they govern**. So an agent (or a human)
working in `src/auth/` has no deterministic way to ask "which recorded decisions
govern this code?" This item closes that gap — the discovery differentiator — and
it is also the precondition for the drift gate in `freshness-and-drift-detection`
(you cannot flag a decision "suspect" when its code changes until the decision
knows which code it governs).

The authoring vocabulary already exists as recorded intent:
`rac-decision-applies-to-scope` (traceability gap 7, under the
`relationship-vocabulary` programme) proposes the optional `## Applies To`
section on decisions, with path entries existence-checked by
`rac relationships --validate`. This roadmap **adopts** that requirement as
Initiative 1 rather than minting a duplicate vocabulary; shipping it here
also closes the decision-scope gap on that programme's ledger.

The declared scope is the join that feeds both the Explorer's "decisions for
this area" surfacing (ADR-028) and the drift gate — freshness phase 2
consumes it for code-scope drift. The scope reference is authored once and
read everywhere; no parallel vocabulary appears.

## Outcomes

- A decision can declare the code paths or components it governs via
  `## Applies To`, validated like any other reference: path entries
  existence-checked, component names recorded labels.
- Given a file or directory, a deterministic lookup returns the governing
  decisions and their status — CLI and MCP, within the response budget — so
  an agent editing code is grounded in the decisions that constrain it
  without searching.
- One declared vocabulary serves every consumer: the lookup, the Explorer
  surfacing, and freshness phase-2 code-scope drift all join on the same
  reference.
- The answer is a pure function of declared references and the file tree:
  no code parsing, no embeddings, no database.

## Initiatives

### Initiative 1 — Code-scope declaration vocabulary (`rac-decision-applies-to-scope`)

Adopts the existing Proposed requirement as-is: an optional `## Applies To`
section on decisions, path entries existence-checked by
`rac relationships --validate`, component names recorded without
resolution, additive (ADR-007), classification-neutral. Around it, this
roadmap fixes the phase boundaries: literal paths and directories are the
validated form; glob entries are accepted as declared match patterns whose
matching semantics belong to the lookup (Initiative 2) — whether globs are
additionally existence-checked ("matches at least one file") is recorded as
an assumption to revisit, advisory-first if added. Extending the section to
non-decision artifact types (requirements or prompts declaring code scope)
is a later phase. Delivery here also ticks the gap-7 item on the
`relationship-vocabulary` programme's ledger.

### Initiative 2 — Deterministic path→decisions lookup (`rac-path-decisions-lookup`)

A new read-only CLI subcommand and, on MCP, an additive optional `path`
argument on the existing `find_decisions` tool — holding the five-tool
surface rather than adding a sixth (the `lean-context-delivery` budget
tension). The result is a pure function of declared references plus the
file tree (ADR-066): deterministic stdlib glob matching with no
filesystem-order dependence; an empty result is a valid answer; the
response reports which decisions bind and their status, never a judgement
(ADR-034); the response budget holds (ADR-033). The pinned tool-description
battery (ADR-030) is revised additively and deliberately in the same
change — under its own decision if the pinned-surface posture requires
ratification, never silently.

### Initiative 3 — Downstream joins: Explorer surfacing and the drift-gate handoff

No requirement of its own. Records that the declared scope plus the lookup
become the join the Explorer consumes for "decisions for this area"
(ADR-028) and that `freshness-and-drift-detection` phase 2 consumes for
code-scope drift — so the scope reference is authored once and read
everywhere.

## Constraints

- Declared and validated, never inferred (ADR-065, ADR-074): code scope is
  authored, human-reviewed reference data; no parsing of code content, no
  similarity, no embeddings (ADR-066).
- Deterministic and offline (ADR-002): lookup output is a pure function of
  declared references and the file tree; results are deterministically
  ordered; no database — associations resolve at read time (ADR-080),
  stateless per call (ADR-032).
- Additive only (ADR-007): a new CLI face and a new optional MCP argument;
  `find_decisions` without a path is byte-identical to today; the MCP
  surface stays at five tools and the budget holds (ADR-033).
- ADR-030's pinned descriptions change only as a deliberate, additive
  revision with the battery updated in the same change.
- The lookup reports which decisions bind — never a compliance judgement
  (ADR-034).

## Non-Goals

- Parsing or understanding code semantics; this maps declared paths, not
  meaning.
- Auto-associating decisions to code by similarity or embeddings.
- A sixth MCP tool.
- Enforcing scope coverage — no "every decision must declare scope" gate.
- Component-id registry or resolution semantics — component names stay
  recorded labels this cycle.
- Non-decision artifact types declaring code scope (later phase, per
  Initiative 1).

## Success Measures

- The requirement's evidence decisions (adr-018, adr-023, adr-027, adr-033)
  declare checkable `## Applies To` scope and validation checks the path
  entries.
- Querying a governed path returns its governing decisions deterministically
  and reproducibly — byte-identical across runs and platforms, with the CLI
  and MCP payloads consistent.
- An ungoverned path, or one outside the repository, returns a valid empty
  result, not an error.
- The Explorer and the drift gate consume the same declared reference when
  they land — no second vocabulary appears.
- `rac validate rac/`, `rac relationships rac/ --validate`, and
  `rac review rac/` stay clean.

## Assumptions

- Declared path scopes are precise enough to be useful without code
  analysis, the same way `## Related` references are useful without
  semantic linking.
- Literal-path existence-checking plus lookup-time glob matching covers the
  real need; glob existence-checking can be added later, advisory-first, if
  stale globs prove noisy.
- Component identifiers stay unresolved labels until a real consumer needs
  registry semantics — a later decision, not pre-decided here.

## Risks

- Path globs drift as the codebase is refactored, leaving stale scopes.
  Mitigation: exactly the drift the freshness gate is meant to surface — a
  moved path becomes a "suspect" signal, not silent rot; literal paths are
  existence-checked at PR time.
- Revising the pinned `find_decisions` description destabilises agents
  pinned by the ADR-030 battery. Mitigation: additive-only revision, the
  battery updated in the same change, under its own decision if required.
- Over-broad scopes (`src/**` on everything) make the lookup noisy.
  Mitigation: the response names the matching declared entry so
  over-breadth is visible in review; no engine heuristic filtering
  (ADR-034).

## Related Decisions

- adr-002
- adr-007
- adr-019
- adr-028
- adr-030
- adr-032
- adr-033
- adr-034
- adr-065
- adr-066
- adr-074
- adr-080
- adr-087
- adr-093
- adr-094

## Related Roadmaps

- deterministic-substrate
- freshness-and-drift-detection
- relationship-vocabulary
- lean-context-delivery

## Related Requirements

- rac-decision-applies-to-scope
- rac-path-decisions-lookup

## Related Designs

- code-scope-consumption

## Related Tickets

- itsthelore/asdecided-core#273
