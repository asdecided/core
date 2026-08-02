---
schema_version: 1
id: RAC-01K8Q7MCP411
type: decision
---
# ADR-128: Hard MCP Response Budgets

## Context

ADR-033 established a deterministic character budget for MCP tool payloads,
but the first implementation retained two overrun cases: summaries were only
marked, and deep relationship neighborhoods could remain far above the
configured limit. The stdio binary also had no way to choose a startup budget,
so the documented configurability existed only in an embedding API.

Those exceptions make the safety property illusory. An agent receiving a
24,000-character summary or a 60,000-character graph response has still had
its context flooded, even if the payload says `truncated: true`. The native
AsDecided server must enforce the boundary at the point where every transport
serializes a tool result.

## Decision

Every successful MCP tool payload string is at or below its effective
character budget.

- The default is 10,000 characters. `decided-mcp --budget N` configures the
  server-wide budget for both stdio and HTTP, and `N` must be at least 128.
- The `get_artifact` and `retrieve_grounding` per-call `budget` arguments may
  lower the startup value. A positive value below 128 is rejected as a tool
  error; a caller cannot use it to produce an invalid oversized response.
- Repeated collections (`matches`, `items`, `incoming`, `neighborhood`,
  `decisions`, `attention`, and outgoing relationship targets) are reduced
  deterministically from the tail at whole-item boundaries. Retrieval
  excerpts and artifact content may use a deterministic fitting prefix before
  whole items are removed.
- `truncated`, `omitted`, and `hint` remain the only response markers. Omitted
  counts are truthful: they include source overflow and entries removed by the
  budget pass, while character-prefix reductions report dropped characters
  where the source shape supports that count.
- If fixed fields alone cannot fit, the serializer returns a small structured
  `response_budget_exceeded` error instead of an oversized successful payload.
- These rules replace the historical summary and deep-neighborhood overrun
  exceptions. Port parity does not preserve those context-flooding bugs.

ADR-033 remains the governing rationale for deterministic character budgets;
this decision hardens its implementation contract for the native server.

## Status

Accepted

## Category

Technical

## Consequences

Agents receive a predictable upper bound on every successful result, across
both transports and every response shape. Broad summaries and graph walks may
return less context than before, but the marker and hint make the omission
visible and actionable. Operators can choose a larger bound deliberately at
startup without changing the default safety posture.

There is no cursor or session state: callers narrow the query or make another
stateless request when they need omitted context. A 128-character minimum
leaves room for the structured budget error and avoids accepting unusably tiny
server configurations.

## Alternatives Considered

### Preserve the oracle's overrun exceptions

Rejected. Marking an oversized success does not protect an agent context, and
the Rust cutover is the opportunity to retire the known bug-for-bug behavior.

### Remove the configurable startup budget

Rejected. Different clients and deployment contexts need different context
windows; a deterministic operator-controlled character limit is cheap to
support and remains transport-neutral.

### Use token counts or an LLM judge

Rejected. Tokenizers vary by model and version, and model judgement would
break deterministic local enforcement. Character counts and fixed truncation
rules preserve ADR-032 and ADR-066.

## Code Constraints

```yaml
version: 1
eligibility: eligible
reason: "The native budget boundary and explicit overflow response are stable source contracts."
rules:
  - id: mcp-budget-keeps-default
    kind: require_pattern
    path_glob: "rust/rac-engine/src/budget.rs"
    pattern: 'pub const DEFAULT_BUDGET: i64 = 10_000;'
    message: "The native MCP response budget must retain its 10,000-character default."
  - id: mcp-budget-keeps-minimum
    kind: require_pattern
    path_glob: "rust/rac-engine/src/budget.rs"
    pattern: 'pub const MIN_BUDGET: i64 = 128;'
    message: "Configured response budgets must retain the explicit 128-character minimum."
  - id: mcp-budget-has-explicit-overflow
    kind: require_pattern
    path_glob: "rust/rac-engine/src/budget.rs"
    pattern: 'pub const BUDGET_ERROR: &str = "response_budget_exceeded";'
    message: "Fixed-field budget failures must return an explicit structured error."
  - id: mcp-budget-is-configurable-at-startup
    kind: require_pattern
    path_glob: "rust/decided-mcp/src/main.rs"
    pattern: '"--budget"'
    message: "The native MCP server must expose the response budget at startup."
```

## Related Decisions

- adr-007
- adr-032
- adr-033
- adr-066
- adr-121

## Related Requirements

- rac-agent-context-guide

## Applies To

- rust/rac-engine/src/budget.rs
- rust/decided-mcp/src/main.rs
- rust/decided-mcp/src/http.rs
- rust/decided-mcp/src/tools.rs
- rust/decided-mcp/tests/response_budget.rs
- rust/decided-mcp/tests/http_transport.rs
- docs/mcp.md
- rust/PORT-CONTRACT.d/10-mcp-surface.md
