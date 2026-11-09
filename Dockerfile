# One static-ish binary on musl, then an image with nothing else in it.
FROM rust:1-alpine AS build

RUN apk add --no-cache musl-dev cmake make perl
WORKDIR /src

COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY spec ./spec
RUN cargo build --release --locked

FROM alpine:3.22

# rustls verifies Henrri's certificate against the system store.
RUN apk add --no-cache ca-certificates && \
    adduser -S -u 65532 -H -D henrri

LABEL io.modelcontextprotocol.server.name="io.github.edouard-claude/henrri-mcp" \
      org.opencontainers.image.source="https://github.com/edouard-claude/henrri-mcp" \
      org.opencontainers.image.description="Unofficial MCP server for the Henrri invoicing API" \
      org.opencontainers.image.licenses="MIT"

COPY --from=build /src/target/release/mcp-henrri /usr/local/bin/mcp-henrri

USER 65532
ENV HENRRI_DOWNLOAD_DIR=/tmp/henrri
# stdio by default; pass --http 0.0.0.0:8080 (with HENRRI_HTTP_TOKEN) to serve HTTP.
ENTRYPOINT ["/usr/local/bin/mcp-henrri"]
