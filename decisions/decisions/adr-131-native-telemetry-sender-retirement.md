---
schema_version: 1
id: RAC-01K8Q7MCP431
type: decision
---
# ADR-131: Native Telemetry Is Local-Only

## Status

Accepted

## Category

Product

## Context

ADR-041 defined an opt-in anonymous usage ping for the original Python
implementation. The native Rust cutover carries the consent record and the
`decided telemetry` compatibility command, but it does not contain a network
client or a sender. A compiled-in PostHog key therefore creates a misleading
security and procurement signal even though no request can be made.

The product's current trust boundary is local-first and zero-egress. Public
documentation, CLI output, and source comments must describe the shipped
native binary rather than the historical sender design.

## Decision

ADR-041 is amended for the native engine:

- `decided telemetry` remains a local consent and status record for compatibility
  with existing configuration and the local usage read-back commands.
- The native engine sends no telemetry and contains no outbound telemetry
  sender. There is no daily ping, endpoint, retry loop, or network side channel.
- The native build carries no PostHog write key. The consent record is retained
  only as local state until a separately approved replacement is implemented.
- `decided usage --share` and `decided mcp-stats --share` remain explicit,
  user-reviewed URL builders; they do not transmit anything automatically.
- Any future remote collection requires a new decision covering its data flow,
  sender, consent, and enterprise/air-gap behavior. It must not be restored by
  reintroducing the retired key or by changing documentation alone.

ADR-086's enterprise lock remains valid: it prevents opt-in and records the
operator's hard-lock choice, but it is no longer the control that prevents a
native network request because the native sender does not exist.

## Consequences

The native binary's zero-egress claim is now directly reflected in its CLI,
documentation, and source. Existing consent files remain readable and safe to
remove. The old ADR-041 payload and PostHog design remain historical context;
they are not a promise about the native release line.

The retention signal described by ADR-041 is not available from the native
binary. If that signal becomes necessary, it must be designed and reviewed as
a new product surface rather than silently revived.

## Related Decisions

- adr-040
- adr-041
- adr-046
- adr-086

## Applies To

- rust/rac-engine/src/consent.rs
- rust/rac-engine/src/commands.rs
- docs/cli.md
- docs/index.md
- docs/security.md
