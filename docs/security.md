# Security posture

AsDecided is local-first and deterministic. The native `decided` CLI reads
repository Markdown and emits local files, stdout, and exit codes. Core
validation and retrieval require no hosted index, model call, account, or
Python runtime.

## No-egress boundary

Validation, relationships, review, gate, search, and export operate on the
local filesystem. Optional MCP usage telemetry remains consent-gated and
content-free; regulated installations can hard-disable it with:

```bash
decided telemetry off --enterprise
```

Optional read-access audit records are local files. Shipping them elsewhere is
the operator's separate responsibility.

## Dependency surface

The shipped implementation is the Cargo workspace under `rust/`. Review
`rust/Cargo.lock` for its resolved dependency graph. The Python package,
Python dependency manifest, and Python engine were retired from this
repository; the final snapshot is preserved at `python-engine-final`.

## Verification

```bash
cd rust
cargo test --workspace --release
cargo clippy --workspace --all-targets -- -D warnings
```

CI also runs contract certification against `asdecided-spec` and live-corpus
invariants. This is a self-attested open-source security posture, not a
third-party certification.
