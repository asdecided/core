# decided

Engineering decisions your agents can follow.

`decided` validates repository-local product knowledge, retrieves the decisions
that govern a change, and deterministically enforces machine-checkable decision
constraints against source code. It runs offline: no embeddings, model call,
hosted index, or Python runtime.

```sh
cargo install decided

decided quickstart
decided validate decisions/
decided gate decisions/ --code
```

The read-only MCP server is distributed separately as `decided-mcp` through
[Homebrew and native release archives](https://github.com/asdecided/core#install).

Documentation and source: [`asdecided/core`](https://github.com/asdecided/core)
