# syntax=docker/dockerfile:1.6

FROM rust:latest AS builder
WORKDIR /workspace
COPY . .
RUN rustup install 1.91.0 && \
    rustup default 1.91.0 && \
    cargo build --release --bin runinator-broker

FROM alpine:3.20

RUN addgroup -S runinator && adduser -S -G runinator runinator \
    && apk add --no-cache ca-certificates

WORKDIR /home/runinator
COPY --from=builder /workspace/target/release/runinator-broker /usr/local/bin/runinator-broker
RUN chmod +x /usr/local/bin/runinator-broker

ENV RUST_LOG=info \
    RUNINATOR_BROKER_ADDR=0.0.0.0:7070
EXPOSE 7070
USER runinator
ENTRYPOINT ["/usr/local/bin/runinator-broker"]
