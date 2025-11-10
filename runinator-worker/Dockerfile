# syntax=docker/dockerfile:1.19.0

FROM rust:1.91-bookworm AS builder
WORKDIR /workspace
COPY . .
RUN rustup install 1.91.0 && \
    rustup default 1.91.0 && \
    cargo build --release --bin runinator-worker && \
    cargo build --release -p runinator-plugin-console

FROM debian:bookworm-slim

RUN groupadd --system runinator && useradd --system --gid runinator --home /home/runinator --create-home runinator \
    && apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && mkdir -p /opt/runinator/plugins \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /home/runinator
COPY --from=builder /workspace/target/release/runinator-worker /usr/local/bin/runinator-worker
COPY --from=builder /workspace/target/release/libruninator_plugin_console.so /opt/runinator/plugins/
RUN chown -R runinator:runinator /usr/local/bin/runinator-worker
RUN chmod -R 775 /usr/local/bin/runinator-worker

ENV RUST_LOG=info \
    RUST_BACKTRACE=1
VOLUME ["/data"]
USER runinator
ENTRYPOINT ["/usr/local/bin/runinator-worker"]
