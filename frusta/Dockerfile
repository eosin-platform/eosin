ARG RUNTIME_IMAGE=debian:trixie-slim
ARG BASE_IMAGE=thavlik/histion-base:latest

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
    cargo build --release --bin histion-frusta && \
    cp /app/target/release/histion-frusta /app/histion-frusta && \
    sccache --show-stats

FROM ${RUNTIME_IMAGE}
RUN apt-get update && apt-get install -y libssl3 && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/histion-frusta /usr/local/bin/histion-frusta
CMD ["histion-frusta"]
