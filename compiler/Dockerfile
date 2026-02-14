ARG RUNTIME_IMAGE=debian:trixie-slim
ARG BASE_IMAGE=thavlik/eosin-base:latest

FROM ${BASE_IMAGE} AS builder
COPY common/src common/src
COPY compiler compiler
COPY storage storage
WORKDIR /app/compiler
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,target=/sccache \
    --mount=type=cache,target=/app/target \
    export SCCACHE_SERVER_UDS="/tmp/sccache-${HOSTNAME}-$$.sock" && \
    sccache --start-server && \
    cargo build --release --bin eosin-compiler && \
    cp /app/target/release/eosin-compiler /app/eosin-compiler && \
    sccache --show-stats

FROM ${RUNTIME_IMAGE}
RUN apt-get update && apt-get install -y --no-install-recommends \
    libssl3 \
    ca-certificates \
    libopenslide0 \
    htop \
    && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/eosin-compiler /usr/local/bin/eosin-compiler
CMD ["eosin-compiler"]
