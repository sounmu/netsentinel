# Multi-stage build: compile on the server's native architecture (no cross-compilation)
FROM rust:1.94.1 AS builder

WORKDIR /build
COPY Cargo.toml Cargo.lock ./

# Build dependencies only (cached until Cargo.toml/Cargo.lock change)
RUN mkdir src && echo "fn main() {}" > src/main.rs \
    && cargo build --release \
    && rm -rf src

# Build actual source
COPY src/ ./src/
RUN touch src/main.rs && cargo build --release && strip target/release/network-monitor-server

# Minimal runtime image
FROM ubuntu:24.04

RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY --from=builder /build/target/release/network-monitor-server /app/server

EXPOSE 3000
CMD ["/app/server"]
