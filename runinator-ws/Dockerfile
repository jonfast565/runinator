# syntax=docker/dockerfile:1.6

FROM rust:1.90.1 AS builder
WORKDIR /workspace
COPY . .
RUN cargo build --release --bin runinator-ws

FROM debian:bookworm-slim AS runtime
RUN useradd --system --uid 10001 --gid 100 --create-home runinator
WORKDIR /home/runinator
COPY --from=builder /workspace/target/release/runinator-ws /usr/local/bin/runinator-ws
ENV RUST_LOG=info
EXPOSE 8080
USER runinator
ENTRYPOINT ["/usr/local/bin/runinator-ws"]
