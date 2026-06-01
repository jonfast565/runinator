FROM rust:1.91.1-bookworm AS builder
WORKDIR /workspace
COPY . .
RUN rustup install 1.91.1 && \
    rustup default 1.91.1 && \
    cargo build --release --bin runinator-broker

FROM debian:bookworm-slim

RUN groupadd --system runinator && useradd --system --gid runinator --home /home/runinator --create-home runinator \
    && apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /home/runinator
COPY --from=builder /workspace/target/release/runinator-broker /usr/local/bin/runinator-broker
RUN chown -R runinator:runinator /usr/local/bin/runinator-broker
RUN chmod -R 775 /usr/local/bin/runinator-broker

ENV RUST_LOG=info \
    RUST_BACKTRACE=1 \
    RUNINATOR_BROKER_TRANSPORT=tcp \
    RUNINATOR_BROKER_ADDR=0.0.0.0:7070
EXPOSE 7070
USER runinator
ENTRYPOINT ["/usr/local/bin/runinator-broker"]
