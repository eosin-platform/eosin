ARG RUNTIME_IMAGE=debian:trixie-slim
ARG BASE_IMAGE=thavlik/histion-base:latest

FROM ${BASE_IMAGE} AS builder
COPY common/src common/src
COPY compiler compiler
COPY storage storage
WORKDIR /app/compiler
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,target=/sccache \
    --mount=type=cache,target=/app/target \
    sccache --start-server && \
    cargo build --release --bin histion-compiler && \
    cp /app/target/release/histion-compiler /app/histion-compiler && \
    sccache --show-stats

FROM ${RUNTIME_IMAGE}
RUN apt-get update && apt-get install -y --no-install-recommends \
    libssl3 \
    libopenslide0 \
    && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/histion-compiler /usr/local/bin/histion-compiler
CMD ["histion-compiler"]
