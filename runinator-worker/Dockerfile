# syntax=docker/dockerfile:1.6

FROM rust:1.81 AS builder
WORKDIR /workspace
COPY . .
RUN cargo build --release --bin runinator-worker && \
    cargo build --release -p runinator-plugin-console

FROM debian:bookworm-slim AS runtime
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && rm -rf /var/lib/apt/lists/*
RUN useradd --system --uid 10001 --gid 100 --create-home runinator
WORKDIR /home/runinator
COPY --from=builder /workspace/target/release/runinator-worker /usr/local/bin/runinator-worker
COPY --from=builder /workspace/target/release/libruninator_plugin_console.so /opt/runinator/plugins/
ENV RUST_LOG=info
VOLUME ["/data"]
USER runinator
ENTRYPOINT ["/usr/local/bin/runinator-worker"]
