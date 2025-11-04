# syntax=docker/dockerfile:1.6

FROM rust:1.81 AS builder
WORKDIR /workspace
COPY . .
RUN cargo build --release --bin runinator-importer

FROM debian:bookworm-slim AS runtime
RUN useradd --system --uid 10001 --gid 100 --create-home runinator
WORKDIR /home/runinator
COPY --from=builder /workspace/target/release/runinator-importer /usr/local/bin/runinator-importer
COPY runinator-importer/tasks /opt/runinator/tasks
ENV RUST_LOG=info
USER runinator
ENTRYPOINT ["/usr/local/bin/runinator-importer"]
