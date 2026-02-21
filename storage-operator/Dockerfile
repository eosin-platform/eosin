ARG RUNTIME_IMAGE=debian:trixie-slim
ARG BASE_IMAGE=thavlik/eosin-base:latest

FROM ${BASE_IMAGE} AS builder
COPY common/src common/src
COPY types/src types/src
COPY operator/src operator/src
WORKDIR /app/operator
RUN cargo build --release --bin eosin-operator

FROM ${RUNTIME_IMAGE}
COPY --from=builder /app/target/release/eosin-operator /usr/local/bin/eosin-operator
CMD ["eosin-operator"]
