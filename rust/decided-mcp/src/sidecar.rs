//! Audit (ADR-084), telemetry (ADR-040) and the usage ping (ADR-041) — the
//! side channels the wire contract pins as NEVER touching stdout bytes
//! (PORT-CONTRACT.d/10 §7: identical call sequences with audit on vs off are
//! frame-for-frame byte-identical; the one designed exception, audit
//! `on_write_error: block`, is out of scope for this port).
//!
//! This module is the documented seam: `observe` wraps every tool call while
//! keeping the wire payload unchanged. The JSONL audit recorder is implemented
//! in `audit.rs` (config-driven via `.decided/config.yaml`). The native engine
//! has no telemetry sender or network side channel (ADR-131), so observation
//! remains a no-op around the read-only protocol.

/// The no-op observation seam: time-and-record hooks would wrap `call` here.
pub fn observe<F: FnOnce() -> String>(_tool: &str, call: F) -> String {
    call()
}
