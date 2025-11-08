# syntax=docker/dockerfile:1.6

FROM rust:1.91-bookworm AS builder
WORKDIR /workspace
COPY . .
RUN rustup install 1.91.0 && \
    rustup default 1.91.0 && \
    cargo build --release --bin runinator-broker

FROM debian:bookworm-slim

RUN groupadd --system runinator && useradd --system --gid runinator --home /home/runinator --create-home runinator \
    && apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /home/runinator
COPY --from=builder /workspace/target/release/runinator-broker /usr/local/bin/runinator-broker
RUN chmod +x /usr/local/bin/runinator-broker

ENV RUST_LOG=info \
    RUNINATOR_BROKER_ADDR=0.0.0.0:7070
EXPOSE 7070
USER runinator
ENTRYPOINT ["/usr/local/bin/runinator-broker"]

