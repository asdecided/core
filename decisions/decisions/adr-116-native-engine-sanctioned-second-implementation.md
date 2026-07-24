---
schema_version: 1
id: RAC-KXGVR299XY5E
type: decision
tags: [architecture, engine, rust, parity, clients, ci]
---
# ADR-116: The Native Rust Engine Is a Sanctioned Second Implementation Under Lockstep Guards

## Context

ADR-063 set the rule that made RAC's determinism promise defensible: there is
**one** deterministic engine (Python, `rac.core`), non-Python clients are thin
clients over the stable contract, and a native re-implementation is an explicit
exception — undertaken only when a concrete need outweighs the cost, and only
under two guards (a shared, language-neutral spec file both engines read, and a
cross-language conformance suite proving output parity). ADR-063 recorded that
no native port was undertaken at that time.

That exception has since been fully exercised, on the record and on this branch.
A native Rust engine was built across two efforts — the engine spike
(roadmap:native-engine-spike) and the derived-index port
(roadmap:native-derived-index) — and held to byte-identity against the frozen
Python oracle, not similarity:

- **CLI byte-parity**: 130/130 mainline + 391/391 closure + 44/44 retrieve +
  45/45 index cases — identical stdout bytes and exit codes, human / `--json` /
  `--sarif`, over the live corpus and fixtures, each run twice with
  byte-identical scoreboards.
- **MCP wire parity**: 56/56 vs the mainline server and 76/76 vs the
  retrieval-branch server, cache-off; 52/52 and 71/71 cache-on; plus a
  mutation-sequence referee — every frame byte-identical.
- **Fuzzing**: ~13,000 distinct inputs, ~120,000 engine-pair executions, 9
  engine bugs found and fixed, zero unexplained divergences remaining. The
  fuzzer also surfaced 30+ inputs that crash the *original*.
- **The recorded index architecture** (ADR-099–112) ported byte-identical at
  the store level — stronger than the read-model-contract fallback the roadmap
  allowed.
- **Performance**: warm `find --json` 43 ms vs the Python cache's 446 ms and an
  8.7 s cache-free walk; serving startup 35 ms vs 2.2 s. A log-scale gap, same
  bytes out at every rung.

Crucially, ADR-063's two guards are now **closed**, with evidence on this branch:

- **Guard 1 — shared spec (closed).** One language-neutral registry is the
  single source of truth both engines read: the Python engine loads
  `ARTIFACT_SPECS` from `src/rac/spec/artifact-specs.json` and the Rust engine
  embeds the same bytes; itsthelore/asdecided-spec hosts the canonical
  `schema/artifact-specs.json` upstream and rac-core vendors it, with a sync
  gate enforcing equality. Recorded in ADR-115; landed at commit `bdd369c`, sync
  gate at `40557f2`, proven behavior-neutral (full Python suite green, every
  byte-parity surface unchanged).
- **Guard 2 — conformance suite (closed).** rac-spec hosts a cross-language
  output-parity conformance tier (`conformance/output-parity.json` +
  `conformance/vectors/`): byte-for-byte golden stdout and exit codes for
  deterministic commands over the example corpora. Both engines — the Python
  reference and the Rust port — reproduce every case. The rac-core certification
  runner landed at commit `40557f2`; both engines certified 11/11 locally
  against a rac-spec checkout.

With the concrete need demonstrated (zero-Python-install CLI, in-editor and
serving latency) and both guards satisfied, ADR-063's deferred question — should
a native engine exist — is now answerable.

## Decision

The native Rust engine is adopted as a **sanctioned second implementation** of
the RAC engine, coexisting with the Python reference in a hybrid topology, under
lockstep guards that make drift structurally impossible. This supersedes
ADR-063's "no native port" posture while preserving its thin-client rule for
every *other* language.

1. **Two engines, one contract, one arbiter.** The Python reference
   implementation remains the **arbiter** of behavior and the source of truth
   for the contract (as rac-spec's `conformance.md` already states). The Rust
   engine is a conformant re-implementation, never an independent fork of
   behavior; where the two disagree, the Python reference wins until the
   disagreement is adjudicated — and where the reference and rac-spec disagree,
   that is a spec defect to be filed, not silently followed.

2. **Guards are load-bearing, not ceremonial.** Guard 1 (ADR-115) and Guard 2
   remain permanent conditions of the two-engine arrangement, not one-time
   entry checks. The shared spec file and the conformance suite are the
   mechanisms by which a rule added to the reference is reflected in the port —
   or CI goes red.

3. **CI enforces lockstep as a required merge gate.** On `main`, the pre-merge
   tier (ADR-075, ADR-027) runs, and blocks the merge on: the byte-parity
   batteries (CLI / closure / retrieve / index, and MCP cache-on and cache-off)
   oracle-vs-Rust; the Guard 1 sync gate (`rust/spec/sync_spec.py`); and the
   Guard 2 conformance certification of both engines against rac-spec
   (`rust/tools/conformance_certify.py`). Both gates are currently inert, gated
   on `RAC_SPEC_DIR`; **wiring rac-spec in as a fetchable CI dependency and
   setting `RAC_SPEC_DIR` is the activation step** that makes them live.

4. **Per-surface engine choice; fenced surfaces stay Python.** The topology
   permits each delivery surface to choose its engine. The Rust engine is
   adopted where parity is proven: the CLI's covered command set and the
   six-tool stdio MCP surface. Surfaces deliberately *not* ported remain
   Python-only until separately addressed: the Explorer TUI (ADR-028), document
   `ingest` (markitdown sidecar, ADR-072), and HTTP MCP transport (ADR-098,
   stdio-only in the port). Adopting a surface natively requires its own parity
   evidence.

5. **Distribution.** The Python reference ships as today (PyPI, `rac-core`). The
   Rust engine ships as a compiled `rac` / `rac-mcp` binary per platform, built
   with its version string compiled in (retiring the `RAC_RS_VERSION` spike
   seam). A user installs the native binary for speed; the Python reference
   stays installable and is the arbiter. The two are interchangeable on the
   covered surface by construction.

6. **Retrieval-branch sequencing — the port follows, never leads.** The
   grounding-retrieval surface (roadmap:grounding-retrieval-surface) is ported
   and parity-proven (44/44), but its one existing-surface byte change (the root
   argparse choices list gains `retrieve`) is **not adopted until that branch
   merges** into the reference. The Rust engine tracks the reference's merged
   state; it does not ship an existing-surface change ahead of the Python engine.

7. **Adopted behavior is bug-for-bug, with a closed list of deliberate
   divergences.** The Rust engine reproduces the reference's behavior exactly,
   *except* a small, documented set where it diverges in the contract's favor —
   each already on the record, each with an upstream follow-up:
   - **Oracle-crash class** — unhashable YAML keys, constructor/tag mismatches,
     out-of-range timestamps crash the Python engine uncaught; the Rust engine
     reports a marked `internal-oracle-divergence` instead. Recommendation:
     report, don't crash — and fix the Python engine separately. These inputs
     are unreachable from any valid corpus.
   - **Duplicate-token df** — the Python cache-on path dedups a repeated query
     term's document frequency where its own cold path counts per occurrence
     (an ADR-112 byte-neutrality violation). The Rust engine keeps warm == cold;
     duplicate-token cases are excluded from cache-on referees until the Python
     defect is fixed upstream, after which the exclusion drops.
   - **S5 accepted miss** — an in-place rewrite preserving both size and
     mtime_ns is invisible to the fast freshness rung in *both* engines; the
     `--verify` content-confirm floor catches it. Pinned as-is.
   - **inotify deferred (ADR-114)** — the freshness tracker's fastest rung is
     the stat-manifest scan; behavior-neutral, a latency-only seam left for a
     later decision.

8. **Other-language clients remain thin.** ADR-063's core principle stands for
   every language other than the one sanctioned here: a TypeScript SDK, an
   editor extension, or any other non-Python client is a thin client over the
   contract (`--json`, `export`, exit codes, MCP), not a re-implementation. The
   Rust engine is *the* single native exception, justified by the concrete needs
   ADR-063 named; it is not a precedent that licenses a third engine.

## Consequences

Determinism is preserved by construction rather than by discipline: the shared
spec, the conformance suite, and the merge-gated parity batteries make it
impossible to land a reference behavior the port silently lacks, or vice versa.
The determinism promise ADR-063 protected now rests on *enforced equivalence
between two engines* rather than on there being only one.

The cost is two engine codebases. That cost is bounded: the reference stays
small, the shared spec removes the largest drift surface, and CI — not human
vigilance — is what catches divergence. The benefit is a step change in the
performance ceiling (the log-scale ladder) and a zero-Python-install path for
the CLI and serving.

Trade-offs accepted: per-platform binary distribution is new operational
surface; the fenced surfaces (Explorer, ingest, HTTP transport) carry a Python
dependency until separately ported, so a deployment using them is not
Python-free; and three deliberate divergences plus one shared accepted miss are
carried as documented debts with upstream Python fixes owed. The retrieval
surface's adoption is deferred to its branch merge, so the port briefly trails
the reference there by design.

Activation is not automatic: until rac-spec is a fetchable CI dependency and
`RAC_SPEC_DIR` is set, the Guard 1 and Guard 2 gates skip. Landing that CI
wiring is the concrete next step this decision depends on.

## Status

Accepted

## Category

Architecture

## Supersedes

- ADR-063

## Alternatives Considered

- **Keep ADR-063 as-is; treat the Rust engine as a permanent experiment.**
  Rejected: the port is byte-parity-proven across the covered surface and the
  guards are closed; leaving it unsanctioned would mean carrying a fully-built,
  contract-conformant engine with no recorded status, and forgoing its
  performance and zero-install benefits for no determinism gain.
- **Flip authority to Rust — make the native engine the source of truth.**
  Rejected here: the Python reference is the arbiter the specification and
  conformance suite are written against, and it is where the fenced surfaces
  still live. A hybrid with the reference as arbiter keeps determinism enforced
  while leaving a later authority flip available once every surface is ported.
- **PyO3 bindings — one Rust core, consumed in-process by the Python server.**
  A strong single-source end state (ADR-031's in-process rule holds literally),
  and compatible with this decision as a *later* step; not adopted now because
  it makes the binding layer new contract surface and is unnecessary to sanction
  the two-engine arrangement the evidence already supports.
- **Native-light: port parse/classify, delegate validation to Python.**
  Rejected for the same reason ADR-063 rejected it — a partial re-implementation
  is the worst of both, a real drift surface plus a Python dependency — and the
  full port has removed the premise.

## Related Decisions

- adr-063
- adr-062
- adr-064
- adr-092
- adr-075
- adr-027
- adr-072
- adr-098
- adr-112
- adr-114
- adr-115

## Related Roadmaps

- native-engine-spike
- native-derived-index
