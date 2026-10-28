# syntax=docker/dockerfile:1
FROM oven/bun:1.4.2 AS web-build
WORKDIR /src/web
COPY web/package.json web/bun.lock ./
RUN bun install --frozen-lockfile --ignore-scripts
COPY web/ ./
RUN bun run build

FROM rust:1.99.1-trixie AS server-build
WORKDIR /src
COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY crates/ crates/
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,target=/src/target,sharing=locked \
    cargo build --release --locked -p stream-server && \
    cp target/release/stream-server /usr/local/bin/stream-server

FROM debian:trixie-slim AS runtime
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates curl openssl && \
    rm -rf /var/lib/apt/lists/* && \
    groupadd --gid 10001 mello && \
    useradd --uid 10001 --gid mello --no-create-home mello && \
    install -d -o mello -g mello /data
WORKDIR /app
COPY --from=server-build /usr/local/bin/stream-server /usr/local/bin/stream-server
COPY --from=web-build /src/web/dist /app/web
COPY --chmod=755 docker/entrypoint.sh /usr/local/bin/mello-entrypoint
ENV STREAM_HOST=0.0.0.0 STREAM_PORT=8080 STREAM_DB_PATH=/data STREAM_STATIC_DIR=/app/web RUST_LOG=info
USER 10001:10001
EXPOSE 8080
HEALTHCHECK --interval=30s --timeout=5s --start-period=15s --retries=3 \
    CMD curl --fail --silent --output /dev/null "http://127.0.0.1:${STREAM_PORT}/health/readyz" || exit 1
ENTRYPOINT ["/usr/local/bin/mello-entrypoint"]
CMD ["stream-server"]
