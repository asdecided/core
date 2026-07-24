# Official native AsDecided container image.
#
# Local build:
#   docker build -t rac .
#   docker run --rm -v "$PWD:/work" asdecided validate decisions/
FROM rust:1.94-bookworm AS builder

COPY rust /src/rust
WORKDIR /src/rust
RUN cargo build --release --locked -p decided -p decided-mcp

FROM debian:bookworm-slim

ARG DECIDED_VERSION=dev
LABEL org.opencontainers.image.title="rac" \
      org.opencontainers.image.description="RAC (Lore) requirements-as-code CLI" \
      org.opencontainers.image.source="https://github.com/itsthelore/asdecided-core" \
      org.opencontainers.image.licenses="Apache-2.0" \
      org.opencontainers.image.version="${DECIDED_VERSION}"

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates git \
    && rm -rf /var/lib/apt/lists/*
COPY --from=builder /src/rust/target/release/decided /usr/local/bin/decided
COPY --from=builder /src/rust/target/release/decided-mcp /usr/local/bin/decided-mcp

WORKDIR /work
ENTRYPOINT ["decided"]
CMD ["--help"]
