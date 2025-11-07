# syntax=docker/dockerfile:1.6

FROM rust:1.90.1 AS builder
WORKDIR /workspace
COPY . .
RUN cargo build --release --bin runinator-broker

FROM debian:bookworm-slim AS runtime
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && rm -rf /var/lib/apt/lists/*
RUN useradd --system --uid 10001 --gid 100 --create-home runinator
WORKDIR /home/runinator
COPY --from=builder /workspace/target/release/runinator-broker /usr/local/bin/runinator-broker
ENV RUST_LOG=info \
    RUNINATOR_BROKER_ADDR=0.0.0.0:7070
EXPOSE 7070
USER runinator
ENTRYPOINT ["/usr/local/bin/runinator-broker"]
