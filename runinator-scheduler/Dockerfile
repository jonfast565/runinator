# syntax=docker/dockerfile:1.6

FROM rust:1.90.1 AS builder
WORKDIR /workspace
COPY . .
RUN cargo build --release --bin runinator-scheduler

FROM debian:bookworm-slim AS runtime
RUN useradd --system --uid 10001 --gid 100 --create-home runinator
WORKDIR /home/runinator
COPY --from=builder /workspace/target/release/runinator-scheduler /usr/local/bin/runinator-scheduler
ENV RUST_LOG=info
USER runinator
ENTRYPOINT ["/usr/local/bin/runinator-scheduler"]
