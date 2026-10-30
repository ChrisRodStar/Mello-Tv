# syntax=docker/dockerfile:1
FROM rust:1.99.1-trixie@sha256:53766005847cbe02c3f59bfa49c1c4bb16695f2c9e5829c3d576a15406f5e733 AS build
WORKDIR /src
COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY crates/ crates/
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,target=/src/target,sharing=locked \
    cargo build --release --locked -p stream-server && \
    cp target/release/mello-jellyfin /usr/local/bin/mello-jellyfin

FROM debian:trixie-slim@sha256:89c1ea91e897f0fe0a3731595330bc45b8bdef4047464a07116f891a94054b31
LABEL org.opencontainers.image.title="mello-jellyfin" \
      org.opencontainers.image.description="On-demand stream resolver and .strm library generator for Jellyfin"
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates curl && \
    rm -rf /var/lib/apt/lists/* && \
    install -d -m 0777 /data /library
COPY --from=build /usr/local/bin/mello-jellyfin /usr/local/bin/mello-jellyfin
ENV MELLO_HOST=0.0.0.0 MELLO_PORT=8080 MELLO_DATA_DIR=/data MELLO_LIBRARY_DIR=/library RUST_LOG=info
# Override with `user:` in compose so files match your Jellyfin user.
USER 1000:1000
EXPOSE 8080
VOLUME ["/data", "/library"]
HEALTHCHECK --interval=30s --timeout=5s --start-period=15s --retries=3 \
    CMD curl --fail --silent --max-time 4 --output /dev/null "http://127.0.0.1:${MELLO_PORT}/health" || exit 1
ENTRYPOINT ["/usr/local/bin/mello-jellyfin"]
