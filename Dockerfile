# Official native AsDecided container images.
#
# CLI (the default target):
#   docker build -t asdecided .
#   docker run --rm -v "$PWD:/work" asdecided validate decisions/
#
# MCP server (used by the official MCP Registry and Docker MCP Catalog):
#   docker build --target asdecided-mcp -t asdecided-mcp .
#   docker run --rm -i -v "$PWD:/work" asdecided-mcp --root /work
FROM rust:1.94-bookworm AS builder

COPY rust /src/rust
WORKDIR /src/rust
RUN cargo build --release --locked -p decided -p decided-mcp

FROM debian:bookworm-slim AS runtime

ARG DECIDED_VERSION=dev
LABEL org.opencontainers.image.title="AsDecided" \
      org.opencontainers.image.description="Deterministic engineering decisions for coding agents" \
      org.opencontainers.image.source="https://github.com/asdecided/core" \
      org.opencontainers.image.licenses="Apache-2.0" \
      org.opencontainers.image.version="${DECIDED_VERSION}"

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates git \
    && rm -rf /var/lib/apt/lists/*
COPY --from=builder /src/rust/target/release/decided /usr/local/bin/decided
COPY --from=builder /src/rust/target/release/decided-mcp /usr/local/bin/decided-mcp
COPY LICENSE NOTICE THIRD-PARTY-NOTICES /usr/share/doc/asdecided/

WORKDIR /work

FROM runtime AS asdecided-mcp
LABEL io.modelcontextprotocol.server.name="io.github.asdecided/core"
ENTRYPOINT ["decided-mcp"]

FROM runtime AS asdecided-cli
ENTRYPOINT ["decided"]
CMD ["--help"]
