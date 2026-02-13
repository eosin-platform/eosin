ARG RUNTIME_IMAGE=debian:trixie-slim
ARG BASE_IMAGE=thavlik/eosin-base:latest

FROM ${BASE_IMAGE} AS builder
COPY common/src common/src
COPY storage storage
COPY frusta frusta
WORKDIR /app/frusta
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,target=/sccache \
    --mount=type=cache,target=/app/target \
    sccache --start-server && \
    cargo build --release --bin eosin-frusta && \
    cp /app/target/release/eosin-frusta /app/eosin-frusta && \
    sccache --show-stats

FROM ${RUNTIME_IMAGE}
RUN apt-get update && apt-get install -y libssl3 && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/eosin-frusta /usr/local/bin/eosin-frusta
CMD ["eosin-frusta"]
