ARG RUNTIME_IMAGE=debian:trixie-slim
ARG BASE_IMAGE=thavlik/histion-base:latest

FROM ${BASE_IMAGE} AS builder
COPY common/src common/src
COPY meta meta
WORKDIR /app/meta
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,target=/sccache \
    --mount=type=cache,target=/app/target \
    sccache --start-server && \
    cargo build --release --bin histion-meta && \
    cp /app/target/release/histion-meta /app/histion-meta && \
    sccache --show-stats

FROM ${RUNTIME_IMAGE}
RUN apt-get update && apt-get install -y libssl3 && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/histion-meta /usr/local/bin/histion-meta
CMD ["histion-meta"]
