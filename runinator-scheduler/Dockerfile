# syntax=docker/dockerfile:1.6

FROM rust:latest AS builder
WORKDIR /workspace
COPY . .
RUN rustup install 1.91.0 && \
    rustup default 1.91.0 && \
    cargo build --release --bin runinator-scheduler

FROM alpine:3.20

RUN addgroup -S runinator && adduser -S -G runinator runinator \
    && apk add --no-cache ca-certificates

WORKDIR /home/runinator
COPY --from=builder /workspace/target/release/runinator-scheduler /usr/local/bin/runinator-scheduler
RUN chmod +x /usr/local/bin/runinator-scheduler

ENV RUST_LOG=info
USER runinator
ENTRYPOINT ["/usr/local/bin/runinator-scheduler"]
