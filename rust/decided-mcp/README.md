# decided-mcp

`decided-mcp` gives agents read-only access to the engineering decisions stored
in an AsDecided repository. It runs locally and deterministically: no hosted
index, embeddings, model call, or Python runtime is required.

```sh
cargo install decided-mcp
decided-mcp --root /path/to/repository
```

Example client configuration:

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

The server supports MCP over stdio by default and reads the corpus from the
current directory unless `--root` is supplied. Documentation and source:
[`asdecided/core`](https://github.com/asdecided/core).

mcp-name: io.github.asdecided/core
