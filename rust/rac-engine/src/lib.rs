//! decided-engine — experimental Rust port of the decided-core engine
//! (roadmap:native-engine-spike). The Python tree at `src/` is the frozen
//! oracle; the binding behavior contract is `rust/PORT-CONTRACT.md`.
//!
//! Module map (keep cross-module surface minimal):
//! - `pycompat`: CPython string/float/round/repr semantics, table-driven
//!   from the packaged `assets/spec/pycompat-tables.json`.
//! - `pyjson`: the Python `json.dumps`-shaped writer (indent=2 and JSONL).
//! - `frontmatter`: bounded PyYAML-1.1 SafeLoader subset.
//! - `markdown`: CommonMark block-boundary tokenizer (headings + inline raw).
//! - `spec`: artifact specs embedded from the vendored rac-spec registry.
//! - `walk`: corpus discovery in component-wise sorted-path order.
//! - `parse`: file -> parsed artifact (frontmatter + sections + fields).
//! - `classify`: deterministic classification over specs.
//! - `identity`: artifact identifiers and the id grammar.
//! - `validate`: structural validation and the finding catalog.
//! - `relationships`: edge extraction, resolution, validation issues.
//! - `resolve`: BM25F + RRF search with pinned f64 operation order.
//! - `gitinfo`: git-derived recency/staleness via the real git CLI.
//! - `budget`: the ADR-033 per-response character budget and truncation.
//! - `consent`: the ADR-041/086 sharing-consent record (telemetry.json).
//! - `telemetry`: Guide telemetry log read-back (ADR-040, mcp-stats).
//! - `usage`: CLI usage log read-back + consent-gated recorder (ADR-046).
//! - `sha256`: FIPS 180-4 digest (eval corpus/query-set hashes).
//! - `skill`: bundled agent skills — registry + embedded-asset install.
//! - `hook`: bundled git hooks — registry + embedded-asset install.
//! - `eval`: the ADR-066 grounding retrieval benchmark and gate.
//! - `portal`: the vendored Portal shell + export-HTML assembly.
//! - `agent_rules`: the ADR-067 agent-rules projection and drift gate.
//! - `okf`: the ADR-048 OKF bundle projection (git-derived recency join).
//! - `revisions`: ADR-043 git-revision materialization (`git archive` + tar).
//! - `compare`: repository state comparison (watchkeeper's load/compare).
//! - `intent`: deterministic intent findings over a comparison.
//! - `watchkeeper`: the watchkeeper report and review verdict.
//! - `output`: human/JSON/SARIF renderers per command.
//! - `commands`: CLI command entry points (argv already parsed).
//! - `federation`: strict offline parent-manifest and byte-snapshot verification.
//! - `federated_corpus`: one verified source-aware composition loader.
//! - `cli`: argv parsing and exit codes matching the oracle's argparse
//!   surface (PORT-CONTRACT.d/01).

pub mod pycompat;
pub mod pyjson;
pub mod frontmatter;
pub mod markdown;
pub mod spec;
pub mod walk;
pub mod parse;
pub mod classify;
pub mod identity;
pub mod corpus;
pub mod composition;
pub mod graph_composition;
pub mod validate;
pub mod relationships;
pub mod diff;
pub mod inspect;
pub mod improve;
pub mod stats;
pub mod resolve;
pub mod retrieve;
pub mod gitinfo;
pub mod budget;
pub mod portfolio;
pub mod index;
pub mod index_format;
pub mod derived;
pub mod index_store;
pub mod derived_cache;
pub mod read_model;
pub mod parallel_build;
pub mod delta_generation;
pub mod freshness;
mod freshness_watch;
pub mod coverage;
pub mod review;
pub mod gate;
pub mod herald;
pub mod sentry;
pub mod doctor;
pub mod mdhtml;
pub mod export;
pub mod federation;
pub mod federation_generation;
pub mod federated_corpus;
pub mod portal;
pub mod agent_rules;
pub mod okf;
pub mod consent;
pub mod telemetry;
pub mod timing;
pub mod usage;
pub mod sha256;
pub mod skill;
pub mod hook;
pub mod eval;
pub mod scaffold;
pub mod rename;
pub mod revisions;
pub mod compare;
pub mod intent;
pub mod watchkeeper;
pub mod output;
pub mod commands;
pub mod cli;
