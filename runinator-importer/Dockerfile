# syntax=docker/dockerfile:1.6

FROM rust:1.91-bookworm AS builder
WORKDIR /workspace
COPY . .
RUN rustup install 1.91.0 && \
    rustup default 1.91.0 && \
    cargo build --release --bin runinator-importer

FROM debian:bookworm-slim

RUN groupadd --system runinator && useradd --system --gid runinator --home /home/runinator --create-home runinator \
    && apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && mkdir -p /opt/runinator/tasks \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /home/runinator
COPY --from=builder /workspace/target/release/runinator-importer /usr/local/bin/runinator-importer
COPY runinator-importer/tasks /opt/runinator/tasks
RUN chmod +x /usr/local/bin/runinator-importer

ENV RUST_LOG=info
USER runinator
ENTRYPOINT ["/usr/local/bin/runinator-importer"]

