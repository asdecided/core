# Contributing to AsDecided

## Local setup

The toolchain is pinned by `rust/rust-toolchain.toml`.

```bash
git clone https://github.com/itsthelore/asdecided-core.git
cd rac-core/rust
rustup show
cargo build --workspace --locked
```

## Verify a change

```bash
cd rust
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --release

target/release/decided validate ../decisions/
target/release/decided relationships ../decisions/ --validate
```

The small Python programs under `rust/tools/` and `rust/spec/` are repository
verification utilities. They are not a Python product engine or installable
SDK. Cross-repository compatibility is certified against
[`asdecided-spec`](https://github.com/itsthelore/asdecided-spec).

## Documentation expectations

Update `docs/` when command behavior, flags, output, or exit codes change.
User-visible changes also need an entry under **Unreleased** in `CHANGELOG.md`.
Machine-readable output is a stable, versioned contract.

## Product-knowledge expectations

Behavioral changes should trace to an artifact under `decisions/`. Keep the
corpus green with `decided validate`, `decided relationships --validate`, and
`decided review`.

## Commit conventions

Follow `decisions/prompts/rac-agent-commit-guidelines.md`:

```text
<type>(<area>): <imperative summary> [roadmap:vX.Y.Z]
```

## License and sign-off

RAC is licensed under Apache-2.0. Contributions must carry a
[Developer Certificate of Origin](https://developercertificate.org/) sign-off:

```bash
git commit -s
```
