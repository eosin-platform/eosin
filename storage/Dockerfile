ARG RUNTIME_IMAGE=debian:trixie-slim
ARG BASE_IMAGE=thavlik/eosin-base:latest

FROM ${BASE_IMAGE} AS builder
COPY common/src common/src
COPY storage storage
WORKDIR /app/storage
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,target=/sccache \
    --mount=type=cache,target=/app/target \
    export SCCACHE_SERVER_UDS="/tmp/sccache-${HOSTNAME}-$$.sock" && \
    sccache --start-server && \
    cargo build --release --bin eosin-storage && \
    cp /app/target/release/eosin-storage /app/eosin-storage && \
    sccache --show-stats

FROM ${RUNTIME_IMAGE}
RUN apt-get update && apt-get install -y libssl3 && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/eosin-storage /usr/local/bin/eosin-storage
CMD ["eosin-storage"]
