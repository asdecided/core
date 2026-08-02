//! Audit (ADR-084), telemetry (ADR-040) and the usage ping (ADR-041) — the
//! side channels the wire contract pins as NEVER touching stdout bytes
//! (PORT-CONTRACT.d/10 §7: identical call sequences with audit on vs off are
//! frame-for-frame byte-identical; the one designed exception, audit
//! `on_write_error: block`, is out of scope for this port).
//!
//! This module is the documented seam: `observe` wraps every tool call the
//! way the oracle's `telemetry.observe(audit.observe(...))` nesting does. The
//! JSONL audit recorder is implemented in `audit.rs` (config-driven via
//! `.decided/config.yaml`); the opt-in telemetry log can still plug in here
//! without touching the protocol layer — the payload passes through unchanged
//! by contract. The daily ping (consent + compiled-in key) is deliberately
//! never implemented: a build without it stays wire-identical.

/// The no-op observation seam: time-and-record hooks would wrap `call` here.
pub fn observe<F: FnOnce() -> String>(_tool: &str, call: F) -> String {
    call()
}
