# AsDecided

**Engineering decisions your agents can follow. Build, as decided.**

AsDecided keeps requirements, decisions, designs, roadmaps, and prompts as
typed Markdown in your repository. Its native Rust engine validates that
knowledge, retrieves relevant decisions deterministically, and serves it
read-only to agents over MCP.

No embeddings, model call, hosted index, or Python runtime is required. The
same repository state produces the same answer.

[Product site](https://asdecided.com/) · [Documentation](https://docs.asdecided.com/start-here/) · [Canonical sources](https://asdecided.com/sources) · [Changelog](https://asdecided.com/changelog)

## Install

Install the native engine and MCP server through Homebrew:

```sh
brew install asdecided/tap/asdecided-core
```

Rust users can install the `decided` CLI directly from crates.io:

```sh
cargo install decided
cargo install decided-mcp
```

Windows users can install both native executables through Scoop:

```powershell
scoop bucket add asdecided https://github.com/asdecided/scoop-bucket
scoop install asdecided
```

Native `decided` and `decided-mcp` archives are also published on
[GitHub Releases](https://github.com/asdecided/core/releases).

Python API consumers should use the
[`asdecided/sdk`](https://github.com/asdecided/sdk) client SDK. It talks to the
native engine; this repository does not ship a second Python engine or a PyPI
package.

## Start a repository

```sh
decided quickstart
decided validate decisions/
decided gate decisions/
decided gate decisions/ --code --base origin/main
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

The official MCP Registry identity is `io.github.asdecided/core`.
Its Registry package is the versioned local MCP image under
`ghcr.io/asdecided/core`; Homebrew and Cargo remain direct native installation
paths for the same Rust server.

The OCI server still needs an explicit repository grant:

```sh
docker run --rm -i -v "$PWD:/work:ro" \
  ghcr.io/asdecided/core:mcp-latest --root /work
```

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
[`asdecided-spec`](https://github.com/asdecided/spec). Live-corpus validation is
based on validity, determinism, freshness, and cache/no-cache equality.

Document ingestion remains an ancillary Python connector rather than part of
the core engine. The retired Python engine is preserved for historical review
at the immutable
[`python-engine-final`](https://github.com/asdecided/core/tree/python-engine-final)
tag; it is not maintained or run in normal CI.

## License

Apache-2.0. See [LICENSE](LICENSE) and [NOTICE](NOTICE).

## Governance and continuity

AsDecided core is solo-maintained and supported on a best-effort basis. The
project does not depend on a hosted service: adopters keep their Markdown
corpus in their own git repository, while the Apache-2.0 implementation and
language-neutral specification remain public. See [GOVERNANCE.md](GOVERNANCE.md)
for the maintainer, release, support, and continuity posture.
