---
schema_version: 1
id: RAC-KWGKGGYW7YF7
type: design
---
# SDK Expansion Strategy — Ranked Order for Non-Python Clients

## Status

Proposed

The recorded outcome of a six-persona council (agent-ecosystem developer,
Go-centric platform engineer, JVM/.NET enterprise architect, OSS
maintainer, DevRel adoption strategist, Rust systems developer) convened
to rank the next non-Python SDKs by adoption impact, with adversarial
challenge on the top consensus candidates and a chaired synthesis. This
design records the ranking, the rationale, the preserved dissents, and
the triggers that reorder it — so future SDK work starts from evidence,
not slideware completeness.

## Context

The only non-Python SDK today is the TypeScript thin client
(`rac-sdk/ts/`, npm `@itsthelore/asdecided-sdk`, pre-1.0). ADR-063 fixes *how*
any further client is built — a thin client over the stable contract
(`--json` outputs, `rac export` payloads, exit codes, MCP), shelling out
to the installed `rac` binary, never a native engine port without the two
gates (a language-neutral `ARTIFACT_SPECS` data file and a cross-language
conformance fixture suite). ADR-092 fixes *where* — a sibling subdir in
the single `rac-sdk` repo. Neither decision says *which* languages come
next or in what order; `go/` appears only as an illustrative placeholder.
This design settles that ordering.

## User Need

Non-Python developers evaluating Lore need a first-class way in. Today
they arrive through the language-agnostic surfaces — the `lore` MCP
server from any agent harness, the VS Code/Cursor extension, the GitHub
Actions, and the exported agent-rules files. The open question is which,
if any, language SDK converts more of them next: agent-tooling builders
(TypeScript), CI/platform teams (Go), enterprise platform teams
(JVM/.NET), or the won't-install-Python segment (Rust/WASM).

## Design

The council's ranked order:

1. **Deepen the language-agnostic surfaces — no new language SDK this
   cycle** (5/6 first-place votes, mean composite 8.4/10, survived
   skeptic challenge). Funded workstream, not a pause:
   - an official OCI image bundling the CLI, unlocking GitLab CI,
     Bitbucket Pipes, and Jenkins docker agents with zero SDK code;
   - non-GitHub report formats (GitLab code-quality JSON, JUnit XML
     alongside SARIF) plus GitLab component and Bitbucket pipe wrappers
     mirroring the shipped GitHub Action;
   - MCP tool depth and the existing `future/` backlog
     (`integration-recipe-factory`, `lean-context-delivery`,
     `retrieval-diagnostics`);
   - start both ADR-063 native-port preconditions now — extract
     `ARTIFACT_SPECS` to a language-neutral data file and build the
     cross-language conformance fixture suite, running it against the
     existing TypeScript SDK as its first consumer;
   - drive the TypeScript SDK to a stable 1.0 so any second SDK copies a
     proven surface.
2. **JVM/Kotlin — trigger-gated second.** Build when the JetBrains
   plugin (`rac-editors/jetbrains`, ADR-092) is scheduled: plugin first
   with an in-repo ProcessBuilder client over `rac … --json`, extract to
   `rac-sdk/jvm/` and publish to Maven Central only when a second JVM
   consumer or recorded external demand warrants the ceremony. Ship with
   air-gap install documentation (ADR-086).
3. **Go — designated fast-follower on demand.** Pre-write the small
   design note now (`rac-sdk/go/`, stdlib-only `os/exec` +
   `encoding/json`, module path fixed with `go/vX.Y.Z` subdirectory tags
   per ADR-092, error mapping ported from the TypeScript client); build
   when demand evidence lands — a partner CI/bot integration or repeated
   "call rac from Go" requests.
4. **.NET — wait-for-demand.** Plausible enterprise-adjacent follower
   once the JVM SDK proves the pattern; no ADR names a .NET surface.
5. **Browser/WASM — rejected as an SDK candidate.** It is a second
   engine, which ADR-063 explicitly rejects; its real payoff
   (zero-install) is served by starting the two gates inside rank 1, and
   it returns only via a new ADR, never as an SDK-ranking side effect.
6. **Rust — rejected.** A subprocess crate is anti-idiomatic for exactly
   the audience it targets; the Rust move is the gated native port, a
   separate decision.
7. **Ruby — rejected.** Cheap but pointless: no strategic surface, no
   agent-tooling ecosystem.

Reordering triggers:

| Trigger | Effect |
| --- | --- |
| JetBrains plugin scheduled into a roadmap | JVM/Kotlin work begins as its substrate (rank 2 fires) |
| Recorded Go demand (partner CI/bot integration, repeated requests) | Go SDK ships (rank 3 fires); analogous evidence promotes .NET |
| Enterprise procurement stalls on "no library integration" | JVM trigger fires early |
| Conversion data shows drop-off at `pip install` | Priority shifts to packaging (containers, standalone binaries) over any SDK |
| Adoption stalls at "read-only MCP tools aren't enough" | Re-weight toward write-capable MCP tools or an SDK |
| A community-owned wrapper SDK appears | Adopt or graduate it per ADR-092 rather than fragment the contract surface |
| TS SDK 1.0 + conformance suite operational | Standing precondition for any second SDK satisfied; ranks 2–4 get cheaper |

## Constraints

- ADR-063: every language SDK is a thin client over the stable contract;
  a native port stays gated behind the `ARTIFACT_SPECS` extraction and
  the conformance fixture suite.
- ADR-092: new SDKs land as subdirs of `rac-sdk`; graduation to an own
  repo only on independent community/cadence.
- ADR-007: the `--json` contract is additive-only; every SDK inherits
  every contract change, so each additional SDK multiplies contract-churn
  cost until the conformance suite exists.
- ADR-086: any SDK aimed at enterprise estates needs an air-gap-ready,
  procurement-friendly install story for the transitive Python CLI
  dependency.
- Capacity: effectively a solo-maintainer product; every published SDK
  is a permanent release-checklist liability.

## Rationale

The consensus was structural, not preferential: under the ADR-063
subprocess model every candidate SDK still requires a pip-installed
Python `rac` CLI, so no wrapper removes the actual conversion blocker for
non-Python users — while the surfaces where those users already meet
Lore (MCP, editors, CI, agent-rules) are language-agnostic and shipping.
The TypeScript SDK exists because a first-party consumer (the VS Code
extension) forced it into existence; no second SDK has such a consumer.
The ranking therefore ties each SDK to its consumer or demand signal:
JVM to the JetBrains plugin, Go to a real CI/platform integration, .NET
to recorded enterprise demand. Go scored the highest mean of any real
SDK (6.5/10) without a single first-place vote — the safe second choice
everywhere — which is precisely the profile of a fast-follower, not a
proactive build.

## Alternatives

- **Build Go now** (`go/` placeholder made real): cheapest to build
  (stdlib subprocess wrapper, tag-to-publish), but Go adopters pick Go
  to avoid runtime dependencies, and a library requiring a Python CLI
  converts fewer users than the ecosystem size suggests. Rejected in
  favour of the fast-follower designation.
- **Build JVM/Kotlin now** (enterprise-architect dissent, the only
  non-null first-place vote): the strongest surviving argument is that
  MCP and CI actions are agent- and pipeline-shaped, not a programmatic
  API, and enterprise buyers embed governance tooling as typed Maven
  libraries. Answered with an explicit trigger rather than a build — the
  JetBrains plugin, when scheduled, gets the SDK as its substrate.
- **Browser/WASM TypeScript**: rejected as posed; it violates ADR-063
  and duplicates the engine. Its preconditions are absorbed into rank 1.
- **Rust thin crate**: rejected; scored worst on fit by the Rust persona
  itself. The segment it targets is served only by the gated native
  port.

## Open Questions

- Which packaging form factor (OCI image, standalone binary, `uvx`)
  best removes the `pip install` barrier, and should it precede all CI
  wrapper work?
- What demand-evidence threshold (issue count, named partner, telemetry
  signal per ADR-041/046) formally fires the Go and .NET triggers?
- Does the conformance fixture suite live in `rac-core` (beside the
  contract) or `rac-sdk` (beside its consumers)?

## Related Decisions

- ADR-007
- ADR-062
- ADR-063
- ADR-073
- ADR-086
- ADR-092

## Related Roadmaps

- agnostic-surfaces
- rac-sdk
- integration-recipe-factory
- lean-context-delivery
- retrieval-diagnostics
