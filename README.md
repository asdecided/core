# AsDecided

**Engineering decisions your agents can follow. Build, as decided.**

AsDecided keeps requirements, decisions, designs, roadmaps, and prompts as
typed Markdown in your repository. Its native Rust engine validates that
knowledge, retrieves relevant decisions deterministically, and serves it
read-only to agents over MCP.

No embeddings, model call, hosted index, or Python runtime is required. The
same repository state produces the same answer.

## Install

Install the complete RAC toolchain through Homebrew:

```sh
brew install itsthelore/tap/rac-full
```

Native `decided` and `decided-mcp` archives are also published on
[GitHub Releases](https://github.com/itsthelore/asdecided-core/releases).

`rac-core` is no longer distributed through PyPI. Python API consumers should
use [`itsthelore/asdecided-sdk`](https://github.com/itsthelore/asdecided-sdk), which is a
client SDK rather than a second engine implementation.

## Start a repository

```sh
decided quickstart
decided validate decisions/
decided gate decisions/
```

New repositories use:

```text
.decided/config.yaml
decisions/
```

Existing artifact IDs such as `RAC-ABC123DEF456` are durable identities and do
not change with the product name.

## Migrate an existing repository

Migration is explicit and never runs during an ordinary command:

```sh
decided migrate layout . --dry-run
decided migrate layout .
```

The migration moves `.rac/` to `.decided/` and `rac/` to `decisions/`. It
refuses to overwrite either destination.

## MCP

```json
{
  "mcpServers": {
    "asdecided": {
      "command": "decided-mcp",
      "args": ["--root", "."]
    }
  }
}
```

## Architecture

Rust is the product engine and the only CLI/MCP runtime in this repository.
The authoritative language-neutral compatibility fixtures live in
[`asdecided-spec`](https://github.com/itsthelore/asdecided-spec). Live-corpus validation is
based on validity, determinism, freshness, and cache/no-cache equality.

Document ingestion remains an ancillary Python connector rather than part of
the core engine. The retired Python engine is preserved for historical review
at the immutable
[`python-engine-final`](https://github.com/itsthelore/asdecided-core/tree/python-engine-final)
tag; it is not maintained or run in normal CI.

## License

Apache-2.0. See [LICENSE](LICENSE) and [NOTICE](NOTICE).
