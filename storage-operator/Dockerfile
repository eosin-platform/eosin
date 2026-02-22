ARG RUNTIME_IMAGE=debian:trixie-slim
ARG BASE_IMAGE=thavlik/eosin-base:latest

FROM ${BASE_IMAGE} AS builder
COPY common/src common/src
COPY storage/proto storage/proto
COPY storage/src storage/src
COPY storage/build.rs storage/build.rs
COPY storage-operator/src storage-operator/src
COPY storage-operator/build.rs storage-operator/build.rs
WORKDIR /app/storage-operator
RUN cargo build --release --bin eosin-storage-operator

FROM ${RUNTIME_IMAGE}
COPY --from=builder /app/target/release/eosin-storage-operator /usr/local/bin/eosin-storage-operator
CMD ["eosin-storage-operator"]
