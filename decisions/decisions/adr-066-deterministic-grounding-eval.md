---
schema_version: 1
id: RAC-KV6KFCC8MHTM
type: decision
tags: [eval, determinism, retrieval, ci, benchmark]
---
# ADR-066: Grounding Eval Scoring Is Deterministic — No Embeddings, No LLM Judge

## Status

Accepted

## Category

Technical

## Context

v0.23.0 adds a grounding benchmark (`rac eval`, WS1): it runs a fixed query set
against the real retrieval tools and scores whether the right artifacts come
back. A benchmark gives us a number, and a number invites the usual ways of
producing it — embedding similarity, a vector index, an LLM-as-judge scoring
relevance. Each would make the benchmark *look* more sophisticated.

Each would also poison the property the benchmark exists to defend. RAC core is
deterministic by principle (ADR-002: AI is optional; ADR-034: no semantic
verdicts in core; ADR-037/ADR-038: token-boundary, tiered search). A benchmark
whose *score* depends on an embedding model, a vector store, or a model call is
non-deterministic, network- and key-dependent, and unreproducible across
machines and over time — so it could neither gate CI honestly nor be trusted to
detect a real regression versus model drift. We would be measuring the judge,
not the retrieval.

The benchmark's job is narrow and adversarial-by-design: prove that the
retrieval tools return the relevant artifacts and **never** surface a
must-not-return artifact (a superseded decision presented as current is the
exact failure Lore exists to prevent). That is a structural, countable property,
not a semantic one.

## Decision

Eval scoring is a **pure, deterministic function of `(corpus, query set,
retrieval code)`**.

- **No embeddings, no vector search, no LLM judge** anywhere in the scored path
  — consistent with ADR-002 and ADR-034. The eval runs offline with no API keys.
- Scoring uses set-membership metrics over ranked tool output: Precision@k and
  Recall@k against a declared relevant set, plus a hard-negative violation count
  against a declared must-not-return set. The hard-negative check is a gate, not
  a soft metric.
- Rankings are made byte-stable by a defined tie-break: equal-scored artifacts
  order by **ascending artifact id**, layered on the existing deterministic
  retrieval sort (ADR-037).
- The gated statistic is the `metrics` object only; diagnostic `metadata`
  (timestamps, hashes) and `per_query` detail are excluded from gate comparison
  so a clock never breaks the build. The `metrics` object MUST be byte-identical
  across repeated runs on an unchanged corpus.
- Re-baselining is **human-gated** (`rac eval --update-baseline`); CI never
  updates the baseline.

The eval measures real retrieval (it calls the same code the MCP tools call), so
it guards the product surface rather than a parallel scoring model.

## Consequences

### Positive

- The benchmark is reproducible, offline, and contract-testable byte-for-byte;
  a failing gate means retrieval changed, not that a judge drifted.
- Regression-injection tests can prove the gate is real (a forced recall drop or
  hard-negative violation must fail it deterministically).
- The benchmark reinforces, rather than erodes, the determinism guarantee the
  rest of core depends on.

### Negative

- Set-membership metrics over a curated corpus do not capture graded relevance
  or "good but not labelled" results; the benchmark guards labelled retrieval,
  not subjective quality.
- The corpus and query set must be authored and maintained by hand; their value
  depends on sourcing realistic cases, not tuning numbers.

### Risks

- The fixture corpus drifts from real-world retrieval. Mitigation: grow the
  query set from design-partner usage and treat the in-repo corpus as a
  regression guard, not a vanity score.
- A future contributor adds a "smarter" embedding-based scorer. Mitigation: this
  ADR makes that a superseding decision, not a quiet change; the determinism
  rule is pinned by the byte-stability test.

## Alternatives Considered

### Embedding / vector-similarity scoring

Score relevance by cosine similarity in an embedding space.

#### Disadvantages

- Non-deterministic across model and library versions; requires a model and
  often network; violates ADR-002 and the byte-stable contract.

### LLM-as-judge

Ask a model whether each result is relevant.

#### Disadvantages

- Non-deterministic, costly, key- and network-dependent; measures the judge, not
  the retrieval; embeds the semantic verdict ADR-034 keeps out of core.

### No benchmark — rely on unit tests

Trust existing tests to catch retrieval regressions.

#### Disadvantages

- Unit tests assert individual behaviors; they do not measure aggregate
  retrieval quality or guard against silent ranking regressions and
  superseded-decision leakage across a realistic corpus.

Deterministic set-membership scoring over real tool output is selected.

## Code Constraints

```yaml
version: 1
eligibility: eligible
reason: "The scored path has explicit dependency and hard-negative boundaries."
rules:
  - id: eval-has-no-network-client
    kind: forbid_import
    path_glob: "rust/rac-engine/src/eval.rs"
    pattern: '^(reqwest|hyper|ureq)(::|$)'
    message: "Grounding evaluation must remain local and network-free."
  - id: eval-has-no-model-judge
    kind: forbid_pattern
    path_glob: "rust/rac-engine/src/eval.rs"
    pattern: '(?i)llm[_ -]?judge|openai|anthropic|embedding'
    message: "Grounding evaluation must not introduce a model or embedding judge."
  - id: eval-keeps-hard-negatives
    kind: require_pattern
    path_glob: "rust/rac-engine/src/eval.rs"
    pattern: 'must_not_return'
    message: "The deterministic hard-negative contract must remain in the scored path."
```

## Related Decisions

- adr-002
- adr-034
- adr-037
- adr-038
- adr-007

## Related Requirements

- rac-grounding-eval-benchmark

## Related Roadmaps

- v0.23.0-hardening
