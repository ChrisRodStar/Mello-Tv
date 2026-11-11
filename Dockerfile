# syntax=docker/dockerfile:1
FROM oven/bun:1.4.4@sha256:1d22548f177f4ff38e0c15baa3c2c46c6c2060ba2d1bc573bb4720b407548f1d AS web-build
WORKDIR /src/web
COPY web/package.json web/bun.lock ./
RUN bun install --frozen-lockfile --ignore-scripts
COPY web/index.html web/tsconfig.json web/vite.config.ts ./
COPY web/src/ src/
COPY web/public/ public/
RUN bun run build

FROM rust:1.99.1-trixie@sha256:8a4e90c7946b74626488b1446f712c0fe0ed1a596cbd3d871684bc9c5433ae07 AS server-build
WORKDIR /src
COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY crates/ crates/
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,target=/src/target,sharing=locked \
    cargo build --release --locked -p stream-server && \
    cp target/release/stream-server /usr/local/bin/stream-server

FROM debian:trixie-slim@sha256:f6b08b09a05725f9431502d3b5714f5c4576e82c1a84fa1debe0c24a7e0c3ec4 AS runtime
LABEL org.opencontainers.image.title="Mello TV" \
      org.opencontainers.image.source="https://github.com/ChrisRodStar/Mello-Tv" \
      org.opencontainers.image.description="Self-hosted Mello TV server and web player"
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates curl && \
    rm -rf /var/lib/apt/lists/* && \
    groupadd --gid 10001 mello && \
    useradd --uid 10001 --gid mello --no-create-home --shell /usr/sbin/nologin mello && \
    install -d -o mello -g mello /data
WORKDIR /app
COPY --from=server-build /usr/local/bin/stream-server /usr/local/bin/stream-server
COPY --from=web-build /src/web/dist /app/web
COPY --chmod=755 docker/entrypoint.sh /usr/local/bin/mello-entrypoint
ENV STREAM_HOST=0.0.0.0 STREAM_PORT=8080 STREAM_DB_PATH=/data STREAM_STATIC_DIR=/app/web RUST_LOG=info
USER 10001:10001
EXPOSE 8080
HEALTHCHECK --interval=30s --timeout=5s --start-period=15s --retries=3 \
    CMD curl --fail --silent --max-time 4 --output /dev/null "http://127.0.0.1:${STREAM_PORT}/health/readyz" || exit 1
ENTRYPOINT ["/usr/local/bin/mello-entrypoint"]
CMD ["stream-server"]
