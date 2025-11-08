# syntax=docker/dockerfile:1.6

FROM rust:latest AS builder
WORKDIR /workspace
COPY . .
RUN rustup install 1.91.0 && \
    rustup default 1.91.0 && \
    cargo build --release --bin runinator-worker && \
    cargo build --release -p runinator-plugin-console

FROM alpine:3.20

RUN addgroup -S runinator && adduser -S -G runinator runinator \
    && apk add --no-cache ca-certificates \
    && mkdir -p /opt/runinator/plugins

WORKDIR /home/runinator
COPY --from=builder /workspace/target/release/runinator-worker /usr/local/bin/runinator-worker
COPY --from=builder /workspace/target/release/libruninator_plugin_console.so /opt/runinator/plugins/
RUN chmod +x /usr/local/bin/runinator-worker

ENV RUST_LOG=info
VOLUME ["/data"]
USER runinator
ENTRYPOINT ["/usr/local/bin/runinator-worker"]
