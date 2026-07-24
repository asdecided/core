# Testing

`asdecided-core` is a native Rust workspace. The former Python engine and its pytest
suite are archived at the `python-engine-final` tag and do not run in normal CI.

## Native checks

```bash
cd rust
cargo check --workspace --locked
cargo test --workspace --release
cargo clippy --workspace --all-targets -- -D warnings
```

The committed Rust fixtures cover deterministic command behavior. Contract
compatibility is certified against the language-neutral fixtures in
[`asdecided-spec`](https://github.com/itsthelore/asdecided-spec):

```bash
DECIDED_SPEC_DIR=/path/to/asdecided-spec python rust/spec/sync_spec.py
python rust/tools/conformance_certify.py \
  --spec-dir /path/to/asdecided-spec \
  --engine "rust=$PWD/rust/target/release/decided"
```

The Python programs in `rust/tools/` and `rust/spec/` are test orchestration
utilities, not a product implementation.

## Live-corpus invariants

Normal CI checks the live `decisions/` corpus for validity, determinism,
freshness, and cache/no-cache equality:

```bash
python rust/tools/live_corpus_invariants.py \
  --engine rust/target/release/decided \
  --corpus decisions
```
