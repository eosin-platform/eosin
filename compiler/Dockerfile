FROM histion-base:latest AS builder

WORKDIR /app

COPY Cargo.toml Cargo.lock ./
COPY common common
COPY compiler compiler

RUN cargo build --release --package histion-compiler

FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/histion-compiler /usr/local/bin/

ENTRYPOINT ["histion-compiler"]
