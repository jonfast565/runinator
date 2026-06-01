FROM rust:1.91.1-bookworm AS builder
WORKDIR /workspace
COPY . .
RUN rustup install 1.91.1 && \
    rustup default 1.91.1 && \
    cargo build --release --bin runinator-importer

FROM debian:bookworm-slim

RUN groupadd --system runinator && useradd --system --gid runinator --home /home/runinator --create-home runinator \
    && apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && mkdir -p /home/runinator/.runinator/workflows \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /home/runinator
COPY --from=builder /workspace/target/release/runinator-importer /usr/local/bin/runinator-importer
COPY packs/sdlc/sdlc.wdlp /home/runinator/.runinator/workflows/sdlc.wdlp
COPY packs/sdlc/wdl /home/runinator/.runinator/workflows/wdl
RUN chown -R runinator:runinator /usr/local/bin/runinator-importer
RUN chmod -R 775 /usr/local/bin/runinator-importer

ENV RUST_LOG=info \
    RUST_BACKTRACE=1
USER runinator
ENTRYPOINT ["/usr/local/bin/runinator-importer"]
